// The GL side of the SFX pass (Plan/41 section 16.5, 16.7, pass 12): turns a `DrawList` into draw calls.
//
//   quad runs    one instanced draw per run: the unit quad (4 vertices) x the run's instances, all runs share ONE interleaved
//                instance buffer (a run's attributes are views at an offset, so the batch costs one buffer upload per frame)
//   strip runs   custom-mesh rings: indexed triangles of one dynamic vertex buffer, one draw per run for GLOW (both faces at once,
//                the order of additive faces does not matter), TWO for BLEND (back faces first, then the front ones: the
//                CW then CCW culling passes of the original)
//   fragment     colour = texture, alpha = texture alpha x key alpha; SRCALPHA / INVSRCALPHA (BLEND) or SRCALPHA / ONE (GLOW),
//                depth test on, depth write off, no culling, no fog, no lighting (sfxbase.cpp:2429-2505)
//
// Draw order is fixed by `renderOrder` (transparent objects sort by it before depth), so the list order is the draw order.

import {
  AddEquation,
  BackSide,
  BufferAttribute,
  BufferGeometry,
  CustomBlending,
  DoubleSide,
  DynamicDrawUsage,
  FrontSide,
  GLSL3,
  InstancedBufferGeometry,
  InstancedInterleavedBuffer,
  InterleavedBuffer,
  InterleavedBufferAttribute,
  Mesh,
  OneFactor,
  OneMinusSrcAlphaFactor,
  RawShaderMaterial,
  Scene,
  Sphere,
  SrcAlphaFactor,
  type Side,
  Vector3,
  type Texture,
} from 'three';
import type { RenderPass } from '../passes';
import { QUAD_STRIDE, STRIP_STRIDE, type DrawList, type MeshItem } from './DrawList';
import { releaseSlotGeometry } from './slotGeometry';

const QUAD_VERTEX = /* glsl */ `
precision highp float;
uniform mat4 viewMatrix;
uniform mat4 projectionMatrix;
in vec2 position;
in vec2 uv;
in vec3 iA0;
in vec3 iA1;
in vec3 iA3;
in float iAlpha;
out vec2 vUv;
out float vAlpha;
void main() {
  vec3 p = iA0 * position.x + iA1 * position.y + iA3;
  vUv = uv;
  vAlpha = iAlpha;
  gl_Position = projectionMatrix * viewMatrix * vec4(p, 1.0);
}
`;

const STRIP_VERTEX = /* glsl */ `
precision highp float;
uniform mat4 viewMatrix;
uniform mat4 projectionMatrix;
in vec3 position;
in vec2 uv;
in float aAlpha;
out vec2 vUv;
out float vAlpha;
void main() {
  vUv = uv;
  vAlpha = aAlpha;
  gl_Position = projectionMatrix * viewMatrix * vec4(position, 1.0);
}
`;

const SFX_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
in vec2 vUv;
in float vAlpha;
layout(location = 0) out highp vec4 fragColor;
void main() {
  vec4 t = texture(uMap, vUv);
  fragColor = vec4(t.rgb, t.a * vAlpha);
}
`;

interface SfxMaterial extends RawShaderMaterial {
  uniforms: { uMap: { value: Texture | null } };
}

function material(
  name: string,
  vertex: string,
  additive: boolean,
  side: Side,
  depthWrite = false,
): SfxMaterial {
  return new RawShaderMaterial({
    name,
    glslVersion: GLSL3,
    vertexShader: vertex,
    fragmentShader: SFX_FRAGMENT,
    uniforms: { uMap: { value: null } },
    side,
    transparent: true,
    blending: CustomBlending,
    blendEquation: AddEquation,
    blendSrc: SrcAlphaFactor,
    blendDst: additive ? OneFactor : OneMinusSrcAlphaFactor,
    depthTest: true,
    depthWrite,
  }) as SfxMaterial;
}

/** Places the MESH parts of a frame (REND-19). */
export interface MeshPartHandler {
  /** A new frame: every mesh part not placed again is hidden at `end`. */
  begin(): void;
  /** Draws `item` with the given `renderOrder`. */
  place(item: MeshItem, renderOrder: number): void;
  end(): void;
}

export interface SfxRendererDeps {
  /** The three.js texture of a texture id, `null` when it is not drawable. */
  textureOf(id: number): Texture | null;
  readonly meshes?: MeshPartHandler;
  /** The scene of the pass (the mesh handler needs it before the renderer exists); a new one by default. */
  readonly scene?: Scene;
}

export interface SfxRenderStats {
  /** Draw calls issued by the last `render` (quads and strips; meshes count their own). */
  draws: number;
  quadRuns: number;
  stripRuns: number;
  meshes: number;
}

interface QuadSlot {
  readonly mesh: Mesh;
  readonly geometry: InstancedBufferGeometry;
  readonly material: SfxMaterial;
  /** The instance buffer the attributes were built on, and the run start they point at. */
  buffer: InstancedInterleavedBuffer | null;
  start: number;
}

interface StripSlot {
  readonly mesh: Mesh;
  readonly geometry: BufferGeometry;
  readonly material: SfxMaterial;
}

const QUAD_CORNERS = new Float32Array([-0.5, 0.5, 0.5, 0.5, 0.5, -0.5, -0.5, -0.5]);
const QUAD_UVS = new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]);
const QUAD_INDEX = new Uint16Array([0, 1, 2, 0, 2, 3]);

export class SfxRenderer {
  readonly scene: Scene;
  readonly stats: SfxRenderStats = { draws: 0, quadRuns: 0, stripRuns: 0, meshes: 0 };
  private readonly quadBlend: QuadSlot[] = [];
  private readonly quadGlow: QuadSlot[] = [];
  private readonly stripBlendBack: StripSlot[] = [];
  private readonly stripBlendFront: StripSlot[] = [];
  private readonly stripGlow: StripSlot[] = [];
  private readonly corner = new BufferAttribute(QUAD_CORNERS, 2);
  private readonly cornerUv = new BufferAttribute(QUAD_UVS, 2);
  private readonly cornerIndex = new BufferAttribute(QUAD_INDEX, 1);
  private quadBuffer: InstancedInterleavedBuffer | null = null;
  private quadArray: Float32Array | null = null;
  private stripBuffer: InterleavedBuffer | null = null;
  private stripArray: Float32Array | null = null;
  private stripIndex: BufferAttribute | null = null;
  private stripIndexArray: Uint32Array | null = null;
  private disposed = false;

  constructor(private readonly deps: SfxRendererDeps) {
    this.scene = deps.scene ?? new Scene();
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;
  }

  /** The pass entry (pass 12): the scene through the camera, sorted by `renderOrder`. */
  pass(): RenderPass {
    return {
      name: 'sfx',
      enabled: true,
      render: (ctx) => {
        ctx.device.renderLayer(this.scene, ctx.camera.camera, null, true);
      },
    };
  }

  // ---- quads -------------------------------------------------------------------------------------------------------

  private ensureQuadBuffer(array: Float32Array): InstancedInterleavedBuffer {
    if (this.quadBuffer && this.quadArray === array) return this.quadBuffer;
    // the list grew: the old GL buffer goes with the geometries that reference it, the shared unit quad stays (6b)
    for (const s of [...this.quadBlend, ...this.quadGlow]) {
      releaseSlotGeometry(s.geometry, this.corner, this.cornerUv, this.cornerIndex);
      s.buffer = null;
    }
    const b = new InstancedInterleavedBuffer(array, QUAD_STRIDE, 1);
    b.setUsage(DynamicDrawUsage);
    this.quadBuffer = b;
    this.quadArray = array;
    return b;
  }

  private newQuadSlot(additive: boolean): QuadSlot {
    const geometry = new InstancedBufferGeometry();
    geometry.setAttribute('position', this.corner);
    geometry.setAttribute('uv', this.cornerUv);
    geometry.setIndex(this.cornerIndex);
    geometry.instanceCount = 0;
    // the transparent sort reads the bounding sphere; the instance data are not positions it could derive one from
    geometry.boundingSphere = new Sphere(new Vector3(), 1);
    const mat = material(additive ? 'sfx-glow' : 'sfx-blend', QUAD_VERTEX, additive, DoubleSide);
    const mesh = new Mesh(geometry, mat);
    mesh.frustumCulled = false;
    mesh.matrixAutoUpdate = false;
    mesh.visible = false;
    this.scene.add(mesh);
    return { mesh, geometry, material: mat, buffer: null, start: -1 };
  }

  private bindQuads(
    slot: QuadSlot,
    buffer: InstancedInterleavedBuffer,
    start: number,
    count: number,
  ): void {
    if (slot.buffer !== buffer || slot.start !== start) {
      const o = start * QUAD_STRIDE;
      slot.geometry.setAttribute('iA0', new InterleavedBufferAttribute(buffer, 3, o));
      slot.geometry.setAttribute('iA1', new InterleavedBufferAttribute(buffer, 3, o + 3));
      slot.geometry.setAttribute('iA3', new InterleavedBufferAttribute(buffer, 3, o + 6));
      slot.geometry.setAttribute('iAlpha', new InterleavedBufferAttribute(buffer, 1, o + 9));
      slot.buffer = buffer;
      slot.start = start;
    }
    slot.geometry.instanceCount = count;
  }

  // ---- strips ------------------------------------------------------------------------------------------------------

  private ensureStripBuffers(list: DrawList): void {
    if (this.stripArray === list.stripVerts && this.stripIndexArray === list.stripIndex) return;
    for (const s of [...this.stripBlendBack, ...this.stripBlendFront, ...this.stripGlow]) {
      s.geometry.dispose();
    }
    const vb = new InterleavedBuffer(list.stripVerts, STRIP_STRIDE);
    vb.setUsage(DynamicDrawUsage);
    const ib = new BufferAttribute(list.stripIndex, 1);
    ib.setUsage(DynamicDrawUsage);
    this.stripBuffer = vb;
    this.stripArray = list.stripVerts;
    this.stripIndex = ib;
    this.stripIndexArray = list.stripIndex;
    const bind = (s: StripSlot): void => {
      s.geometry.setAttribute('position', new InterleavedBufferAttribute(vb, 3, 0));
      s.geometry.setAttribute('uv', new InterleavedBufferAttribute(vb, 2, 3));
      s.geometry.setAttribute('aAlpha', new InterleavedBufferAttribute(vb, 1, 5));
      s.geometry.setIndex(ib);
    };
    for (const s of [...this.stripBlendBack, ...this.stripBlendFront, ...this.stripGlow]) bind(s);
  }

  private newStripSlot(additive: boolean, side: Side): StripSlot {
    const geometry = new BufferGeometry();
    geometry.boundingSphere = new Sphere(new Vector3(), 1);
    const vb = this.stripBuffer;
    if (vb) {
      geometry.setAttribute('position', new InterleavedBufferAttribute(vb, 3, 0));
      geometry.setAttribute('uv', new InterleavedBufferAttribute(vb, 2, 3));
      geometry.setAttribute('aAlpha', new InterleavedBufferAttribute(vb, 1, 5));
    }
    if (this.stripIndex) geometry.setIndex(this.stripIndex);
    const mat = material(
      additive ? 'sfx-strip-glow' : 'sfx-strip-blend',
      STRIP_VERTEX,
      additive,
      side,
    );
    const mesh = new Mesh(geometry, mat);
    mesh.frustumCulled = false;
    mesh.matrixAutoUpdate = false;
    mesh.visible = false;
    this.scene.add(mesh);
    return { mesh, geometry, material: mat };
  }

  // ---- the frame ---------------------------------------------------------------------------------------------------

  /** Updates the scene for one frame from the draw list (the geometry of the pass is only valid for this frame). */
  render(list: DrawList): void {
    if (this.disposed) return;
    const stats = this.stats;
    stats.draws = 0;
    stats.quadRuns = 0;
    stats.stripRuns = 0;
    stats.meshes = 0;
    const meshes = this.deps.meshes;
    meshes?.begin();
    let order = 0;
    let qb = 0;
    let qg = 0;
    let sbb = 0;
    let sbf = 0;
    let sg = 0;
    if (list.quads > 0) {
      const buffer = this.ensureQuadBuffer(list.quadData);
      buffer.clearUpdateRanges();
      buffer.addUpdateRange(0, list.quads * QUAD_STRIDE);
      buffer.needsUpdate = true;
    }
    const haveStrips = list.stripIndexCount > 0;
    if (haveStrips) {
      this.ensureStripBuffers(list);
      const vb = this.stripBuffer;
      const ib = this.stripIndex;
      if (vb && ib) {
        vb.clearUpdateRanges();
        vb.addUpdateRange(0, list.stripVertCount * STRIP_STRIDE);
        vb.needsUpdate = true;
        ib.clearUpdateRanges();
        ib.addUpdateRange(0, list.stripIndexCount);
        ib.needsUpdate = true;
      }
    }
    for (const item of list.items) {
      if (item.kind === 'mesh') {
        meshes?.place(item, order++);
        stats.meshes++;
        continue;
      }
      const tex = this.deps.textureOf(item.tex);
      if (!tex) continue;
      if (item.kind === 'quads') {
        const pool = item.additive ? this.quadGlow : this.quadBlend;
        const used = item.additive ? qg++ : qb++;
        let slot = pool[used];
        if (!slot) {
          slot = this.newQuadSlot(item.additive);
          pool.push(slot);
        }
        const buffer = this.quadBuffer;
        if (!buffer) continue;
        this.bindQuads(slot, buffer, item.start, item.count);
        slot.material.uniforms.uMap.value = tex;
        slot.mesh.renderOrder = order++;
        slot.mesh.visible = true;
        stats.quadRuns++;
        stats.draws++;
      } else {
        const passes: [StripSlot[], number, Side][] = item.additive
          ? [[this.stripGlow, sg++, DoubleSide]]
          : [
              [this.stripBlendBack, sbb++, BackSide],
              [this.stripBlendFront, sbf++, FrontSide],
            ];
        for (const [pool, used, side] of passes) {
          let slot = pool[used];
          if (!slot) {
            slot = this.newStripSlot(item.additive, side);
            pool.push(slot);
          }
          slot.geometry.setDrawRange(item.indexStart, item.indexCount);
          slot.material.uniforms.uMap.value = tex;
          slot.mesh.renderOrder = order++;
          slot.mesh.visible = true;
          stats.draws++;
        }
        stats.stripRuns++;
      }
    }
    // the slots not used this frame
    for (let i = qb; i < this.quadBlend.length; i++) hide(this.quadBlend[i]?.mesh);
    for (let i = qg; i < this.quadGlow.length; i++) hide(this.quadGlow[i]?.mesh);
    for (let i = sbb; i < this.stripBlendBack.length; i++) hide(this.stripBlendBack[i]?.mesh);
    for (let i = sbf; i < this.stripBlendFront.length; i++) hide(this.stripBlendFront[i]?.mesh);
    for (let i = sg; i < this.stripGlow.length; i++) hide(this.stripGlow[i]?.mesh);
    meshes?.end();
  }

  /** Pooled GL objects (probe, tests). */
  get slots(): { quad: number; strip: number } {
    return {
      quad: this.quadBlend.length + this.quadGlow.length,
      strip: this.stripBlendBack.length + this.stripBlendFront.length + this.stripGlow.length,
    };
  }

  dispose(): void {
    this.disposed = true;
    for (const s of [...this.quadBlend, ...this.quadGlow]) {
      s.geometry.dispose();
      s.material.dispose();
    }
    for (const s of [...this.stripBlendBack, ...this.stripBlendFront, ...this.stripGlow]) {
      s.geometry.dispose();
      s.material.dispose();
    }
    this.quadBlend.length = 0;
    this.quadGlow.length = 0;
    this.stripBlendBack.length = 0;
    this.stripBlendFront.length = 0;
    this.stripGlow.length = 0;
    this.scene.clear();
  }
}

function hide(m: Mesh | undefined): void {
  if (m) m.visible = false;
}
