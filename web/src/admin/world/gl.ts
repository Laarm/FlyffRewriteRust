// The GPU side of the map (Plan/25 section 3.1: "map canvas (WebGL2)"; 3.4, 3.5): a three.js orthographic view whose units are world metres
// (scene x = world x, scene y = world z, so north is up), the tiles as textured quads, and ONE instanced draw for every entity sprite with a
// per-instance position, size, rotation, sprite rectangle and colour. This module is the only one of the map that touches three.js.
import {
  BufferAttribute,
  BufferGeometry,
  Color,
  DynamicDrawUsage,
  InterleavedBuffer,
  InterleavedBufferAttribute,
  LinearFilter,
  LinearSRGBColorSpace,
  Mesh,
  NoColorSpace,
  NormalBlending,
  OrthographicCamera,
  PlaneGeometry,
  Points,
  Scene,
  ShaderMaterial,
  Texture,
  WebGLRenderer,
} from 'three';
import type { GpuTile } from './tileTextures';
import type { Rect, View } from './view';

/** The attributes of the sprite points (the ones the draw range publishes). */
interface SpriteAttributes {
  readonly interleaved: InterleavedBuffer;
  readonly uv: BufferAttribute;
  readonly col: BufferAttribute;
}

/** Floats per instance in each of the three attribute arrays. */
const VEC4 = 4;
const INITIAL_CAPACITY = 2048;

const TILE_VERT = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

// image row 0 is the north edge: the plane's v = 1 (top) samples the first row of the texture (uploaded without a flip)
const TILE_FRAG = /* glsl */ `
uniform sampler2D map;
uniform float uMax;
varying vec2 vUv;
void main() {
  gl_FragColor = texture2D(map, vec2(vUv.x * uMax, 1.0 - vUv.y));
}`;

// Every sprite is ONE vertex drawn as a point sprite (no instancing, no quad): the position, the edge in CSS pixels and the heading ride in `position`
// and `aRot`, the sheet rectangle and the colour in `aUv` and `aColor`. The fragment shader turns `gl_PointCoord` by the heading (a point is an
// axis-aligned square, so a turning sprite gets a point of 1.414x its edge) and samples the sheet. A point whose centre leaves the canvas disappears
// whole, which the 64 px margin of the culling makes invisible.
const SPRITE_VERT = /* glsl */ `
attribute float aRot;   // heading in radians, clockwise from north
attribute vec4 aUv;     // u0 v0 u1 v1 (v = 0 on the top row of the sheet)
attribute vec4 aColor;
uniform float uDpr;
uniform float uMaxPoint;
varying vec4 vUv;
varying vec4 vColor;
varying float vRot;
varying float vK;
void main() {
  // position.xy = world x and z, position.z = edge in css px
  float k = aRot == 0.0 ? 1.0 : 1.4142136;
  gl_Position = projectionMatrix * viewMatrix * vec4(position.x, position.y, 0.0, 1.0);
  gl_PointSize = min(position.z * k * uDpr, uMaxPoint);
  vUv = aUv;
  vColor = aColor;
  vRot = aRot;
  vK = k;
}`;

const SPRITE_FRAG = /* glsl */ `
uniform sampler2D atlas;
varying vec4 vUv;
varying vec4 vColor;
varying float vRot;
varying float vK;
void main() {
  vec2 p = (gl_PointCoord - 0.5) * vK;
  p.y = -p.y;
  vec2 q = p;
  if (vRot != 0.0) {
    float c = cos(vRot);
    float s = sin(vRot);
    q = vec2(p.x * c - p.y * s, p.x * s + p.y * c);
  }
  q += 0.5;
  if (q.x < 0.0 || q.x > 1.0 || q.y < 0.0 || q.y > 1.0) discard;
  vec4 t = texture2D(atlas, vec2(mix(vUv.x, vUv.z, q.x), mix(vUv.w, vUv.y, q.y)));
  gl_FragColor = vec4(t.rgb * vColor.rgb, t.a * vColor.a);
}`;

/** One tile to draw: its texture, where it goes in the world and in which order (coarse tiles first). */
export interface TileDraw {
  readonly tile: GpuTile;
  readonly rect: Rect;
  /** Draw order: a lower value is drawn first. */
  readonly order: number;
}

/** The instance data of a frame, filled by the engine. */
export interface InstanceBuffers {
  readonly pos: Float32Array;
  readonly uv: Float32Array;
  readonly color: Float32Array;
  readonly capacity: number;
}

export class GlLayer {
  readonly renderer: WebGLRenderer;
  readonly #scene = new Scene();
  readonly #camera = new OrthographicCamera(-1, 1, 1, -1, 0.1, 100);
  readonly #tileGeometry = new PlaneGeometry(1, 1);
  readonly #tileMeshes: { mesh: Mesh; map: { value: Texture }; uMax: { value: number } }[] = [];
  readonly #atlasTexture = new Texture();
  readonly #spriteMaterial: ShaderMaterial;
  readonly #uDpr = { value: 1 };
  readonly #uMaxPoint = { value: 64 };
  readonly #clear = new Color('#cfd5df');
  #sprites: Points;
  #geometry: BufferGeometry;
  #buffers: InstanceBuffers;
  #attrs: SpriteAttributes;
  #lost = false;
  #disposed = false;
  readonly #onLost: (e: Event) => void;
  readonly #onRestored: () => void;
  /** Called when the context is lost and when it is back. */
  onContext: ((lost: boolean) => void) | null = null;

  constructor(canvas: HTMLCanvasElement) {
    this.renderer = new WebGLRenderer({
      canvas,
      antialias: false,
      alpha: false,
      depth: false,
      stencil: false,
      powerPreference: 'high-performance',
    });
    // the tiles and the sprites are written as they are: no colour conversion on the way out (textures are NoColorSpace)
    this.renderer.outputColorSpace = LinearSRGBColorSpace;
    this.renderer.setClearColor(this.#clear, 1);
    this.renderer.autoClear = true;
    this.#camera.position.set(0, 0, 10);
    this.#atlasTexture.flipY = false;
    this.#atlasTexture.generateMipmaps = false;
    this.#atlasTexture.minFilter = LinearFilter;
    this.#atlasTexture.magFilter = LinearFilter;
    this.#atlasTexture.colorSpace = NoColorSpace;
    this.#atlasTexture.premultiplyAlpha = false;
    this.#spriteMaterial = new ShaderMaterial({
      vertexShader: SPRITE_VERT,
      fragmentShader: SPRITE_FRAG,
      uniforms: {
        atlas: { value: this.#atlasTexture },
        uDpr: this.#uDpr,
        uMaxPoint: this.#uMaxPoint,
      },
      transparent: true,
      depthTest: false,
      depthWrite: false,
      blending: NormalBlending,
    });
    this.#buffers = makeBuffers(INITIAL_CAPACITY);
    const made = this.#makeGeometry(this.#buffers);
    this.#geometry = made.geometry;
    this.#attrs = made.attrs;
    this.#sprites = new Points(this.#geometry, this.#spriteMaterial);
    this.#sprites.frustumCulled = false;
    this.#sprites.renderOrder = 10_000;
    this.#scene.add(this.#sprites);

    this.#onLost = (e) => {
      e.preventDefault();
      this.#lost = true;
      this.onContext?.(true);
    };
    this.#onRestored = () => {
      this.#lost = false;
      // the textures died with the context: the sheet is uploaded again, the engine reloads its tiles
      this.#atlasTexture.needsUpdate = true;
      this.onContext?.(false);
    };
    canvas.addEventListener('webglcontextlost', this.#onLost);
    canvas.addEventListener('webglcontextrestored', this.#onRestored);
  }

  /** `true` when the GPU takes BC1 (S3TC) blocks as they are. */
  get s3tc(): boolean {
    return this.renderer.extensions.has('WEBGL_compressed_texture_s3tc');
  }

  get contextLost(): boolean {
    return this.#lost;
  }

  /** The instance arrays the engine writes into. */
  get buffers(): InstanceBuffers {
    return this.#buffers;
  }

  /** Makes room for `count` instances (the arrays are replaced; their content is NOT kept). */
  ensureCapacity(count: number): InstanceBuffers {
    if (count <= this.#buffers.capacity) return this.#buffers;
    let capacity = this.#buffers.capacity;
    while (capacity < count) capacity *= 2;
    this.#scene.remove(this.#sprites);
    this.#geometry.dispose();
    this.#buffers = makeBuffers(capacity);
    const made = this.#makeGeometry(this.#buffers);
    this.#geometry = made.geometry;
    this.#attrs = made.attrs;
    this.#sprites = new Points(this.#geometry, this.#spriteMaterial);
    this.#sprites.frustumCulled = false;
    this.#sprites.renderOrder = 10_000;
    this.#scene.add(this.#sprites);
    return this.#buffers;
  }

  #makeGeometry(b: InstanceBuffers): { geometry: BufferGeometry; attrs: SpriteAttributes } {
    const g = new BufferGeometry();
    // x, z, edge in css px and the heading share one interleaved buffer (the same array the engine fills)
    const interleaved = new InterleavedBuffer(b.pos, VEC4);
    interleaved.setUsage(DynamicDrawUsage);
    const position = new InterleavedBufferAttribute(interleaved, 3, 0);
    const rot = new InterleavedBufferAttribute(interleaved, 1, 3);
    const uv = new BufferAttribute(b.uv, VEC4);
    const col = new BufferAttribute(b.color, VEC4);
    uv.setUsage(DynamicDrawUsage);
    col.setUsage(DynamicDrawUsage);
    g.setAttribute('position', position);
    g.setAttribute('aRot', rot);
    g.setAttribute('aUv', uv);
    g.setAttribute('aColor', col);
    g.setDrawRange(0, 0);
    return { geometry: g, attrs: { interleaved, uv, col } };
  }

  /** Sets the sprite sheet (a canvas); uploaded on the next frame. */
  setAtlas(canvas: HTMLCanvasElement): void {
    this.#atlasTexture.image = canvas;
    this.#atlasTexture.needsUpdate = true;
  }

  setClearColor(css: string): void {
    this.#clear.set(css);
    this.renderer.setClearColor(this.#clear, 1);
  }

  resize(w: number, h: number, dpr: number): void {
    this.#uDpr.value = dpr;
    // the largest point the GPU draws (a point sprite is a square of this many device pixels at most)
    const gl = this.renderer.getContext();
    const range = gl.getParameter(gl.ALIASED_POINT_SIZE_RANGE) as Float32Array | null;
    this.#uMaxPoint.value = Math.max(16, Math.floor(range?.[1] ?? 64));
    this.renderer.setPixelRatio(dpr);
    this.renderer.setSize(Math.max(1, Math.floor(w)), Math.max(1, Math.floor(h)), false);
  }

  setView(v: View): void {
    const hw = (v.w / 2) * v.upp;
    const hh = (v.h / 2) * v.upp;
    const c = this.#camera;
    c.left = v.cx - hw;
    c.right = v.cx + hw;
    c.top = v.cz + hh;
    c.bottom = v.cz - hh;
    c.updateProjectionMatrix();
    c.updateMatrixWorld();
  }

  /** Places the tiles of this frame (the meshes are pooled). */
  setTiles(draws: readonly TileDraw[]): void {
    for (let i = 0; i < draws.length; i += 1) {
      const d = draws[i];
      if (!d) continue;
      let slot = this.#tileMeshes[i];
      if (!slot) {
        const map = { value: d.tile.texture };
        const uMax = { value: 1 };
        const material = new ShaderMaterial({
          vertexShader: TILE_VERT,
          fragmentShader: TILE_FRAG,
          uniforms: { map, uMax },
          transparent: true,
          depthTest: false,
          depthWrite: false,
        });
        const mesh = new Mesh(this.#tileGeometry, material);
        mesh.frustumCulled = false;
        slot = { mesh, map, uMax };
        this.#tileMeshes.push(slot);
        this.#scene.add(mesh);
      }
      slot.map.value = d.tile.texture;
      slot.uMax.value = d.tile.uMax;
      const [x0, z0, x1, z1] = d.rect;
      slot.mesh.position.set((x0 + x1) / 2, (z0 + z1) / 2, 0);
      slot.mesh.scale.set(x1 - x0, z1 - z0, 1);
      slot.mesh.renderOrder = d.order;
      slot.mesh.visible = true;
    }
    for (let i = draws.length; i < this.#tileMeshes.length; i += 1) {
      const slot = this.#tileMeshes[i];
      if (slot) slot.mesh.visible = false;
    }
  }

  /** Publishes the first `count` instances of the buffers to the GPU. */
  setInstanceCount(count: number): void {
    this.#geometry.setDrawRange(0, count);
    const { interleaved, uv, col } = this.#attrs;
    interleaved.clearUpdateRanges();
    uv.clearUpdateRanges();
    col.clearUpdateRanges();
    if (count > 0) {
      interleaved.addUpdateRange(0, count * VEC4);
      uv.addUpdateRange(0, count * VEC4);
      col.addUpdateRange(0, count * VEC4);
    }
    interleaved.needsUpdate = true;
    uv.needsUpdate = true;
    col.needsUpdate = true;
  }

  render(): void {
    if (this.#lost || this.#disposed) return;
    this.renderer.render(this.#scene, this.#camera);
  }

  /** Reads the framebuffer's pixel at CSS (x, y): `[r, g, b, a]` (tests and the probe). */
  readPixel(x: number, y: number): [number, number, number, number] | null {
    const gl = this.renderer.getContext();
    const dpr = this.renderer.getPixelRatio();
    const h = gl.drawingBufferHeight;
    const out = new Uint8Array(4);
    this.render();
    gl.readPixels(
      Math.floor(x * dpr),
      h - 1 - Math.floor(y * dpr),
      1,
      1,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      out,
    );
    return [out[0] ?? 0, out[1] ?? 0, out[2] ?? 0, out[3] ?? 0];
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    const canvas = this.renderer.domElement;
    canvas.removeEventListener('webglcontextlost', this.#onLost);
    canvas.removeEventListener('webglcontextrestored', this.#onRestored);
    for (const t of this.#tileMeshes) (t.mesh.material as ShaderMaterial).dispose();
    this.#tileMeshes.length = 0;
    this.#tileGeometry.dispose();
    this.#geometry.dispose();
    this.#spriteMaterial.dispose();
    this.#atlasTexture.dispose();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}

function makeBuffers(capacity: number): InstanceBuffers {
  return {
    pos: new Float32Array(capacity * VEC4),
    uv: new Float32Array(capacity * VEC4),
    color: new Float32Array(capacity * VEC4),
    capacity,
  };
}
