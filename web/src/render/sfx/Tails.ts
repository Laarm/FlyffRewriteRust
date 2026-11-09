// Ribbons (Plan/41 section 18.1, WP-REND-21; `CTailEffectBelt` / `CTailEffectMng`, TailEffectMng.cpp:23-372, 520-640).
//
// A ribbon is a list of nodes `(pos1, pos2, fade)`: the projectile pushes one pair of points per 60 Hz tick at the HEAD of the list,
// every tick `fade += fadeSpeed` and a node dies at `fade >= 2`. The vertex colour is white with alpha `fade <= 1 ? fade : 2 - fade`
// (the node fades in, then out; the newest node starts transparent), the UV of `pos1` is `(0, ty)`, of `pos2` `(1, ty)` with `ty`
// alternating 0 / 1 along the list, the strip runs newest to oldest (at least two nodes). Texture x vertex colour, SRCALPHA /
// INVSRCALPHA, depth write off, no culling, drawn in pass 10 with an identity world matrix.
//
//   manager   at most 32 ribbons (`MAX_TAILEFFECT`); `AddEffect` fails beyond that
//   belt      at most 512 nodes (`MAX_TAIL`): a push into a full belt is dropped
//   delete    the destructor of the owner deletes the belt AT ONCE (the remaining nodes vanish with the arrow, KEEP)
//
// All points are LEFT-HANDED game space; the strip vertices are mapped to three.js with the `z` negation when they are written.

import {
  BufferGeometry,
  CustomBlending,
  DoubleSide,
  DynamicDrawUsage,
  GLSL3,
  InterleavedBuffer,
  InterleavedBufferAttribute,
  Mesh,
  BufferAttribute,
  RawShaderMaterial,
  Scene,
  Sphere,
  SrcAlphaFactor,
  OneMinusSrcAlphaFactor,
  AddEquation,
  Vector3,
  type Texture,
} from 'three';
import type { RenderPass } from '../passes';

export const MAX_RIBBONS = 32;
export const MAX_NODES = 512;
/** The default `fFadeSpeed` of `AddEffect`. */
export const DEFAULT_FADE_SPEED = 0.03;
/** Floats per strip vertex: `x y z u v alpha`. */
export const TAIL_STRIDE = 6;

export type V3 = readonly [number, number, number];

/** One ribbon. */
export class TailBelt {
  /** Ring of nodes, 7 floats each: `p1 (3), p2 (3), fade`. `head` is the newest. */
  private readonly nodes = new Float32Array(MAX_NODES * 7);
  private head = 0;
  private n = 0;

  constructor(
    /** The canonical path of the texture. */
    readonly texture: string,
    readonly type: number,
    readonly fadeSpeed: number,
  ) {}

  /** Live nodes. */
  get count(): number {
    return this.n;
  }

  /** `CreateTail`: a node pushed at the head with `fade = 0`. A full belt drops the push (returns `false`). */
  push(p1: V3, p2: V3): boolean {
    if (this.n >= MAX_NODES) return false;
    this.head = (this.head + MAX_NODES - 1) % MAX_NODES;
    const o = this.head * 7;
    const a = this.nodes;
    a[o] = p1[0];
    a[o + 1] = p1[1];
    a[o + 2] = p1[2];
    a[o + 3] = p2[0];
    a[o + 4] = p2[1];
    a[o + 5] = p2[2];
    a[o + 6] = 0;
    this.n++;
    return true;
  }

  /** `FrameMove`: ages every node and drops the dead ones (they are always the oldest). */
  step(): void {
    const a = this.nodes;
    for (let i = 0; i < this.n; i++) {
      const o = ((this.head + i) % MAX_NODES) * 7 + 6;
      let f = (a[o] ?? 0) + this.fadeSpeed;
      if (f > 2) f = 2;
      a[o] = f;
    }
    while (this.n > 0) {
      const oldest = ((this.head + this.n - 1) % MAX_NODES) * 7 + 6;
      if ((a[oldest] ?? 0) >= 2) this.n--;
      else break;
    }
  }

  /** The fade value of node `i` (0 = newest). */
  fadeOf(i: number): number {
    return this.nodes[((this.head + i) % MAX_NODES) * 7 + 6] ?? 0;
  }

  /** The two points of node `i` (0 = newest). */
  pointsOf(i: number): [V3, V3] {
    const o = ((this.head + i) % MAX_NODES) * 7;
    const a = this.nodes;
    return [
      [a[o] ?? 0, a[o + 1] ?? 0, a[o + 2] ?? 0],
      [a[o + 3] ?? 0, a[o + 4] ?? 0, a[o + 5] ?? 0],
    ];
  }

  /**
   * Writes the strip vertices (newest to oldest, two per node: `x y z u v alpha` with `z` already mapped to three.js) into `out` at
   * float offset `at` and returns the number of vertices written (0 below two nodes).
   */
  writeStrip(out: Float32Array, at: number): number {
    if (this.n < 2) return 0;
    const a = this.nodes;
    let w = at;
    for (let i = 0; i < this.n; i++) {
      const o = ((this.head + i) % MAX_NODES) * 7;
      const f = a[o + 6] ?? 0;
      const alpha = f > 1 ? 2 - f : f;
      const ty = i & 1;
      out[w++] = a[o] ?? 0;
      out[w++] = a[o + 1] ?? 0;
      out[w++] = -(a[o + 2] ?? 0);
      out[w++] = 0;
      out[w++] = ty;
      out[w++] = alpha;
      out[w++] = a[o + 3] ?? 0;
      out[w++] = a[o + 4] ?? 0;
      out[w++] = -(a[o + 5] ?? 0);
      out[w++] = 1;
      out[w++] = ty;
      out[w++] = alpha;
    }
    return this.n * 2;
  }
}

/** `g_TailEffectMng`: the ribbons of the world. */
export class TailMng {
  private readonly belts: (TailBelt | null)[] = [];
  private alive = 0;

  /** `AddEffect`: a new ribbon, `null` when 32 exist. */
  add(texture: string, type = 1, fadeSpeed = DEFAULT_FADE_SPEED): TailBelt | null {
    if (this.alive >= MAX_RIBBONS) return null;
    const belt = new TailBelt(texture, type, fadeSpeed);
    const free = this.belts.indexOf(null);
    if (free >= 0) this.belts[free] = belt;
    else this.belts.push(belt);
    this.alive++;
    return belt;
  }

  /** `Delete`: the ribbon disappears at once. */
  remove(belt: TailBelt): void {
    const i = this.belts.indexOf(belt);
    if (i < 0) return;
    this.belts[i] = null;
    this.alive--;
  }

  /** `Process`: one 60 Hz step of every ribbon. */
  step(): void {
    for (const b of this.belts) b?.step();
  }

  clear(): void {
    this.belts.length = 0;
    this.alive = 0;
  }

  get ribbons(): number {
    return this.alive;
  }

  /** Live nodes of all ribbons. */
  get nodes(): number {
    let n = 0;
    for (const b of this.belts) n += b?.count ?? 0;
    return n;
  }

  forEach(fn: (belt: TailBelt) => void): void {
    for (const b of this.belts) if (b) fn(b);
  }
}

// ---- drawing --------------------------------------------------------------------------------------------------------

const VERTEX = /* glsl */ `
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

const FRAGMENT = /* glsl */ `
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

export interface TailRendererDeps {
  /** The three.js texture of a path when it is drawable now, else `null` (loading, failed: the ribbon draws nothing). */
  textureOf(path: string): Texture | null;
}

export interface TailRenderStats {
  /** Draw calls of the last frame (one per texture). */
  draws: number;
  vertices: number;
  ribbons: number;
}

interface Slot {
  readonly mesh: Mesh;
  readonly material: RawShaderMaterial;
}

/** Draws the ribbons of a manager: one dynamic strip buffer, one draw per texture, pass 10. */
export class TailRenderer {
  readonly scene = new Scene();
  readonly stats: TailRenderStats = { draws: 0, vertices: 0, ribbons: 0 };
  private verts = new Float32Array(TAIL_STRIDE * 2 * 1024);
  private index = new Uint32Array(6 * 1024);
  private buffer: InterleavedBuffer | null = null;
  private indexAttr: BufferAttribute | null = null;
  private readonly slots: Slot[] = [];
  private disposed = false;

  constructor(private readonly deps: TailRendererDeps) {
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;
  }

  pass(): RenderPass {
    return {
      name: 'ribbons',
      enabled: true,
      render: (ctx) => {
        if (this.stats.draws > 0) ctx.device.renderLayer(this.scene, ctx.camera.camera, null, true);
      },
    };
  }

  private ensureCapacity(vertexCount: number): void {
    if (vertexCount * TAIL_STRIDE <= this.verts.length) return;
    let cap = this.verts.length / TAIL_STRIDE;
    while (cap < vertexCount) cap *= 2;
    this.verts = new Float32Array(cap * TAIL_STRIDE);
    this.index = new Uint32Array(cap * 3);
    this.dropBuffers();
  }

  private dropBuffers(): void {
    for (const s of this.slots) s.mesh.geometry.dispose();
    this.buffer = null;
    this.indexAttr = null;
  }

  private bind(): void {
    if (this.buffer && this.indexAttr) return;
    const vb = new InterleavedBuffer(this.verts, TAIL_STRIDE);
    vb.setUsage(DynamicDrawUsage);
    const ib = new BufferAttribute(this.index, 1);
    ib.setUsage(DynamicDrawUsage);
    this.buffer = vb;
    this.indexAttr = ib;
    for (const s of this.slots) this.attach(s.mesh.geometry, vb, ib);
  }

  private attach(g: BufferGeometry, vb: InterleavedBuffer, ib: BufferAttribute): void {
    g.setAttribute('position', new InterleavedBufferAttribute(vb, 3, 0));
    g.setAttribute('uv', new InterleavedBufferAttribute(vb, 2, 3));
    g.setAttribute('aAlpha', new InterleavedBufferAttribute(vb, 1, 5));
    g.setIndex(ib);
  }

  private newSlot(): Slot {
    const geometry = new BufferGeometry();
    geometry.boundingSphere = new Sphere(new Vector3(), 1e9);
    if (this.buffer && this.indexAttr) this.attach(geometry, this.buffer, this.indexAttr);
    const material = new RawShaderMaterial({
      name: 'ribbon',
      glslVersion: GLSL3,
      vertexShader: VERTEX,
      fragmentShader: FRAGMENT,
      uniforms: { uMap: { value: null } },
      side: DoubleSide,
      transparent: true,
      blending: CustomBlending,
      blendEquation: AddEquation,
      blendSrc: SrcAlphaFactor,
      blendDst: OneMinusSrcAlphaFactor,
      depthTest: true,
      depthWrite: false,
    });
    const mesh = new Mesh(geometry, material);
    mesh.frustumCulled = false;
    mesh.matrixAutoUpdate = false;
    mesh.visible = false;
    this.scene.add(mesh);
    const slot = { mesh, material };
    this.slots.push(slot);
    return slot;
  }

  /** Fills the buffers from the manager and sets up the draws of the frame. */
  render(mng: TailMng): void {
    if (this.disposed) return;
    const stats = this.stats;
    stats.draws = 0;
    stats.vertices = 0;
    stats.ribbons = 0;
    // group the drawable belts by texture
    const groups = new Map<string, TailBelt[]>();
    let total = 0;
    mng.forEach((b) => {
      if (b.count < 2) return;
      if (!this.deps.textureOf(b.texture)) return;
      let g = groups.get(b.texture);
      if (!g) {
        g = [];
        groups.set(b.texture, g);
      }
      g.push(b);
      total += b.count * 2;
    });
    if (total === 0) {
      for (const s of this.slots) s.mesh.visible = false;
      return;
    }
    this.ensureCapacity(total);
    this.bind();
    const verts = this.verts;
    const index = this.index;
    let used = 0;
    let slotIndex = 0;
    let vtx = 0;
    let idx = 0;
    for (const [path, belts] of groups) {
      const tex = this.deps.textureOf(path);
      if (!tex) continue;
      const slot = this.slots[slotIndex] ?? this.newSlot();
      slotIndex++;
      const firstIndex = idx;
      for (const b of belts) {
        const base = vtx;
        const n = b.writeStrip(verts, vtx * TAIL_STRIDE);
        vtx += n;
        // triangle strip -> triangle list: (i, i+1, i+2) and (i+1, i+3, i+2) alternate in the original's strip order
        for (let i = 0; i + 2 < n; i++) {
          if ((i & 1) === 0) {
            index[idx++] = base + i;
            index[idx++] = base + i + 1;
            index[idx++] = base + i + 2;
          } else {
            index[idx++] = base + i + 1;
            index[idx++] = base + i;
            index[idx++] = base + i + 2;
          }
        }
        stats.ribbons++;
      }
      const u = slot.material.uniforms['uMap'];
      if (u) u.value = tex;
      slot.mesh.geometry.setDrawRange(firstIndex, idx - firstIndex);
      slot.mesh.visible = true;
      stats.draws++;
      used++;
    }
    for (let i = used; i < this.slots.length; i++) {
      const s = this.slots[i];
      if (s) s.mesh.visible = false;
    }
    stats.vertices = vtx;
    const vb = this.buffer;
    const ib = this.indexAttr;
    if (vb && ib) {
      vb.clearUpdateRanges();
      vb.addUpdateRange(0, vtx * TAIL_STRIDE);
      vb.needsUpdate = true;
      ib.clearUpdateRanges();
      ib.addUpdateRange(0, idx);
      ib.needsUpdate = true;
    }
  }

  dispose(): void {
    this.disposed = true;
    for (const s of this.slots) {
      s.mesh.geometry.dispose();
      s.material.dispose();
    }
    this.slots.length = 0;
    this.scene.clear();
  }
}
