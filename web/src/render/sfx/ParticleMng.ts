// The point-sprite particle manager (Plan/41 section 18.2, WP-REND-21; `g_ParticleMng`, ParticleMng.cpp, ref/20 section 11).
//
// Up to 200 particle TYPES, created on first use with room for 8192 particles each; a type owns its texture, its sprite size and its
// gravity flag. One particle: position, velocity, `fade` (1 -> 0), `groundY`. Per 60 Hz tick (`CParticles::Update`)
//
//   fade -= 0.015;  pos += vel;  if the TYPE has gravity: vel.y -= 0.005;  fade < 0 -> 0
//   killed when fade <= 0, or when the type has gravity and pos.y < groundY
//
// KEEP-QUIRK: the gravity flag belongs to the TYPE, and the melee hit sparks set it for their types (0..10) for good, so the wand dots
// of type 0 / 1 fall once a spark of that type was created.
//
// Colours (`CreateParticle`): type 0 `(1, 0.5, 0.5)` fading to `(1, 0.25, 0.25)`; types 1..12 `(0.35, 0.35, 1)` fading to black; types
// 13 and above white fading to black. The vertex colour is `lerp(fadeColor, diffuse, fade)`.
//
// Drawing (`CParticles::Render`): D3D point sprites with `POINTSCALE_C = 1`, so the screen size is `viewportHeight * size / distance`
// pixels (at least one): a sprite has the WORLD size `size * 2 tan(fovY / 2)`, never less than one pixel. Colour = texture x vertex
// colour, blend ONE / ONE, depth test on, depth write off, pass 7. The sprites are camera-facing quads of one instanced draw per type.
//
// Users: the wand bolt of a flying caster (`WAND_ATK_AIR`: type 0 aimed at the active mover, else 1), the melee hit sparks of the
// players near the active mover (`MeleeSparks`).

import {
  AddEquation,
  BufferAttribute,
  CustomBlending,
  DoubleSide,
  DynamicDrawUsage,
  GLSL3,
  InstancedBufferGeometry,
  InstancedInterleavedBuffer,
  InterleavedBufferAttribute,
  Mesh,
  OneFactor,
  RawShaderMaterial,
  Scene,
  Sphere,
  Vector3,
  type Texture,
} from 'three';
import type { RenderPass } from '../passes';
import { releaseSlotGeometry } from './slotGeometry';

export const MAX_PARTICLE_TYPES = 200;
export const TYPE_CAPACITY = 8192;
export const FADE_STEP = Math.fround(0.015);
export const GRAVITY = Math.fround(0.005);
/** Floats per instance: `x y z r g b` (`z` already in three.js space). */
export const PARTICLE_STRIDE = 6;

export type V3 = readonly [number, number, number];

// ---- the type table -------------------------------------------------------------------------------------------------

export interface ParticleTypeSpec {
  /** Canonical path of the texture (`model/texture/...`). */
  readonly texture: string;
  /** `D3DRS_POINTSIZE`. */
  readonly size: number;
}

const TEX = 'model/texture/';
const spec = (file: string, size: number): ParticleTypeSpec => ({
  texture: TEX + file.toLowerCase(),
  size,
});

/** `CParticleMng::CreateParticle`'s switch. */
export function particleSpec(type: number): ParticleTypeSpec {
  switch (type) {
    case 0:
      return spec('Sfx_ItemPatical01.dds', 0.3);
    case 1:
      return spec('etc_Particle2.bmp', 0.2);
    case 2:
      return spec('etc_Particle11.bmp', 0.5);
    case 3:
      return spec('etc_Particle11.bmp', 0.3);
    case 4:
      return spec('etc_Particle11.bmp', 0.2);
    case 5:
    case 6:
    case 7:
    case 8:
    case 9:
      return spec(`Sfx_ItemPatical06-0${String(type - 4)}.dds`, 0.3);
    case 10:
      return spec('etc_ParticleCloud01.bmp', 0.5);
    case 11:
      return spec('etc_ParticleCloud01.bmp', 0.3);
    case 12:
      return spec('etc_ParticleCloud01.bmp', 0.2);
    case 13:
    case 14:
    case 15:
      return spec('etc_Particle11.bmp', [0.15, 0.1, 0.07][type - 13] ?? 0.1);
    case 16:
    case 17:
    case 18:
      return spec('etc_Particle12.bmp', [0.15, 0.1, 0.07][type - 16] ?? 0.1);
    case 19:
    case 20:
    case 21:
      return spec('etc_Particle13.bmp', [0.15, 0.1, 0.07][type - 19] ?? 0.1);
    case 22:
    case 23:
    case 24:
      return spec('etc_Particle14.bmp', [0.15, 0.1, 0.07][type - 22] ?? 0.1);
    case 25:
      return spec('etc_Particle12.bmp', 0.5);
    case 26:
      return spec('etc_Particle12.bmp', 0.3);
    case 27:
      return spec('etc_Particle12.bmp', 0.2);
    default:
      return spec('etc_Particle12.bmp', 0.3);
  }
}

export interface ParticleColors {
  readonly diffuse: V3;
  readonly fade: V3;
}

const RED: ParticleColors = { diffuse: [1, 0.5, 0.5], fade: [1, 0.25, 0.25] };
const BLUE: ParticleColors = { diffuse: [0.35, 0.35, 1], fade: [0, 0, 0] };
const WHITE: ParticleColors = { diffuse: [1, 1, 1], fade: [0, 0, 0] };

export function particleColors(type: number): ParticleColors {
  return type === 0 ? RED : type < 13 ? BLUE : WHITE;
}

// ---- the particles --------------------------------------------------------------------------------------------------

/** One type: the live particles in spawn order (`CParticles`). */
export class ParticleType {
  n = 0;
  gravity = false;
  pos = new Float32Array(0);
  vel = new Float32Array(0);
  fade = new Float32Array(0);
  groundY = new Float32Array(0);
  private cap = 0;
  readonly spec: ParticleTypeSpec;
  readonly colors: ParticleColors;

  constructor(readonly type: number) {
    this.spec = particleSpec(type);
    this.colors = particleColors(type);
  }

  /** Appends a particle; `false` at the capacity (`CreateParticle` fails). */
  add(pos: V3, vel: V3, groundY: number): boolean {
    if (this.n >= TYPE_CAPACITY) return false;
    if (this.n >= this.cap) this.grow();
    const i = this.n++;
    this.pos[i * 3] = pos[0];
    this.pos[i * 3 + 1] = pos[1];
    this.pos[i * 3 + 2] = pos[2];
    this.vel[i * 3] = vel[0];
    this.vel[i * 3 + 1] = vel[1];
    this.vel[i * 3 + 2] = vel[2];
    this.fade[i] = 1;
    this.groundY[i] = groundY;
    return true;
  }

  private grow(): void {
    const cap = Math.min(TYPE_CAPACITY, Math.max(32, this.cap * 2));
    const g3 = (a: Float32Array<ArrayBuffer>): Float32Array<ArrayBuffer> => {
      const b = new Float32Array(cap * 3);
      b.set(a);
      return b;
    };
    const g1 = (a: Float32Array<ArrayBuffer>): Float32Array<ArrayBuffer> => {
      const b = new Float32Array(cap);
      b.set(a);
      return b;
    };
    this.pos = g3(this.pos);
    this.vel = g3(this.vel);
    this.fade = g1(this.fade);
    this.groundY = g1(this.groundY);
    this.cap = cap;
  }

  /** `CParticles::Update`. */
  step(): void {
    const { pos, vel, fade, groundY } = this;
    let w = 0;
    for (let i = 0; i < this.n; i++) {
      let f = Math.fround((fade[i] ?? 0) - FADE_STEP);
      const x = Math.fround((pos[i * 3] ?? 0) + (vel[i * 3] ?? 0));
      const y = Math.fround((pos[i * 3 + 1] ?? 0) + (vel[i * 3 + 1] ?? 0));
      const z = Math.fround((pos[i * 3 + 2] ?? 0) + (vel[i * 3 + 2] ?? 0));
      let vy = vel[i * 3 + 1] ?? 0;
      if (this.gravity) vy = Math.fround(vy - GRAVITY);
      if (f < 0) f = 0;
      if (f <= 0 || (this.gravity && y < (groundY[i] ?? 0))) continue;
      if (w !== i) {
        pos[w * 3] = x;
        pos[w * 3 + 1] = y;
        pos[w * 3 + 2] = z;
        vel[w * 3] = vel[i * 3] ?? 0;
        vel[w * 3 + 1] = vy;
        vel[w * 3 + 2] = vel[i * 3 + 2] ?? 0;
        fade[w] = f;
        groundY[w] = groundY[i] ?? 0;
      } else {
        pos[i * 3] = x;
        pos[i * 3 + 1] = y;
        pos[i * 3 + 2] = z;
        vel[i * 3 + 1] = vy;
        fade[i] = f;
      }
      w++;
    }
    this.n = w;
  }

  /** The vertex colour of particle `i`: `lerp(fadeColor, diffuse, fade)`. */
  colorOf(i: number, out: number[] | Float32Array, at = 0): void {
    const f = this.fade[i] ?? 0;
    const { diffuse: d, fade: c } = this.colors;
    out[at] = c[0] + (d[0] - c[0]) * f;
    out[at + 1] = c[1] + (d[1] - c[1]) * f;
    out[at + 2] = c[2] + (d[2] - c[2]) * f;
  }
}

/** `g_ParticleMng`. */
export class ParticleMng {
  private readonly types = new Map<number, ParticleType>();

  /**
   * `CreateParticle`: one particle of `type` (the type is created on its first use). Returns the type, `null` for an invalid type
   * number or a full type.
   */
  create(type: number, pos: V3, vel: V3, groundY: number): ParticleType | null {
    if (type < 0 || type >= MAX_PARTICLE_TYPES) return null;
    let t = this.types.get(type);
    if (!t) {
      t = new ParticleType(type);
      this.types.set(type, t);
    }
    return t.add(pos, vel, groundY) ? t : null;
  }

  /** `Process`: one 60 Hz step of every type. */
  step(): void {
    for (const t of this.types.values()) t.step();
  }

  clear(): void {
    this.types.clear();
  }

  /** Live particles of every type. */
  get live(): number {
    let n = 0;
    for (const t of this.types.values()) n += t.n;
    return n;
  }

  get typeCount(): number {
    return this.types.size;
  }

  typeOf(type: number): ParticleType | undefined {
    return this.types.get(type);
  }

  forEachType(fn: (t: ParticleType) => void): void {
    for (const t of this.types.values()) fn(t);
  }
}

// ---- melee hit sparks -----------------------------------------------------------------------------------------------

/** Ticks of the hit pause during which a hit makes sparks (`m_nPause`). */
export const SPARK_TICKS = 5;
/** Distance to the active mover within which the sparks of other players are made. */
export const SPARK_RANGE = 32;

/** The particle type of a weapon with upgrade level `option` (`nOption & 0xFF`), without the random part of `+10`. */
export function sparkLevel(option: number): number {
  if (option === 10) return 5;
  if (option === 9) return 4;
  if (option === 8) return 3;
  if (option === 7) return 2;
  return option >= 4 ? 1 : 0;
}

/**
 * The velocity of one spark (ActionMoverState.cpp:1000-1010): a random yaw, an elevation of 15..60 degrees, a speed of 0.10..0.13.
 * `rnd` yields `[0, 1)`.
 */
export function sparkVelocity(rnd: () => number): [number, number, number] {
  const az = (rnd() * 360 * Math.PI) / 180;
  const h = ((15 + rnd() * 45) * Math.PI) / 180;
  const speed = 0.1 + rnd() * 0.03;
  const dist = Math.cos(h) * speed;
  return [Math.sin(az) * dist, Math.sin(h) * speed, -Math.cos(az) * dist];
}

/** What the sparks need of the world: the attacker and the active mover. */
export interface SparkWorld {
  /** The logical position (LH) of a mover; `false` when unknown. */
  posOf(id: number, out: [number, number, number]): boolean;
  /** The mover's yaw in degrees; `NaN` when unknown. */
  angleOf(id: number): number;
  isPlayer(id: number): boolean;
  /** The position of the active mover; `false` before the own player exists. */
  activePos(out: [number, number, number]): boolean;
}

interface Pending {
  readonly attacker: number;
  readonly level: number;
  remaining: number;
}

/** The sparks of the melee hits in progress: one particle per tick for five ticks per hit. */
export class MeleeSparks {
  private readonly pending: Pending[] = [];
  private readonly tmp: [number, number, number] = [0, 0, 0];
  private readonly me: [number, number, number] = [0, 0, 0];

  constructor(
    private readonly mng: ParticleMng,
    private readonly world: () => SparkWorld,
    private readonly rnd: () => number = Math.random,
  ) {}

  /** A melee hit of `attacker` whose weapon has the upgrade `option`. */
  hit(attacker: number, option: number): void {
    this.pending.push({ attacker, level: sparkLevel(option & 0xff), remaining: SPARK_TICKS });
  }

  get active(): number {
    return this.pending.length;
  }

  clear(): void {
    this.pending.length = 0;
  }

  /** One 60 Hz tick; `off` is the `SFXRenderOff` option (no sparks are made, the pause still counts down). */
  tick(off: boolean): void {
    const world = this.world();
    let w = 0;
    for (const p of this.pending) {
      this.spark(world, p, off);
      if (--p.remaining > 0) this.pending[w++] = p;
    }
    this.pending.length = w;
  }

  private spark(world: SparkWorld, p: Pending, off: boolean): void {
    if (off) return;
    const me = this.me;
    if (!world.activePos(me) || !world.isPlayer(p.attacker)) return;
    const at = this.tmp;
    if (!world.posOf(p.attacker, at)) return;
    const dx = at[0] - me[0];
    const dy = at[1] - me[1];
    const dz = at[2] - me[2];
    if (dx * dx + dy * dy + dz * dz >= SPARK_RANGE * SPARK_RANGE) return;
    const a = (world.angleOf(p.attacker) * Math.PI) / 180;
    if (Number.isNaN(a)) return;
    // one unit in front of the attacker (`AngleToVectorXZ`), 0.7 up
    const pos: V3 = [at[0] + Math.sin(a), at[1] + 0.7, at[2] - Math.cos(a)];
    const type = p.level === 5 ? 5 + Math.floor(this.rnd() * 6) : p.level;
    const t = this.mng.create(type, pos, sparkVelocity(this.rnd), me[1]);
    if (t) t.gravity = true;
  }
}

// ---- drawing --------------------------------------------------------------------------------------------------------

const QUAD_CORNERS = new Float32Array([-0.5, 0.5, 0.5, 0.5, 0.5, -0.5, -0.5, -0.5]);
const QUAD_UVS = new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]);
const QUAD_INDEX = new Uint16Array([0, 1, 2, 0, 2, 3]);

const VERTEX = /* glsl */ `
precision highp float;
uniform mat4 viewMatrix;
uniform mat4 projectionMatrix;
uniform vec2 uSize;
in vec2 position;
in vec2 uv;
in vec3 iPos;
in vec3 iColor;
out vec2 vUv;
out vec3 vColor;
void main() {
  vec4 vp = viewMatrix * vec4(iPos, 1.0);
  float world = max(uSize.x, uSize.y * length(vp.xyz));
  vec3 right = vec3(viewMatrix[0][0], viewMatrix[1][0], viewMatrix[2][0]);
  vec3 up = vec3(viewMatrix[0][1], viewMatrix[1][1], viewMatrix[2][1]);
  vec3 p = iPos + (right * position.x + up * position.y) * world;
  vUv = uv;
  vColor = iColor;
  gl_Position = projectionMatrix * viewMatrix * vec4(p, 1.0);
}
`;

const FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
in vec2 vUv;
in vec3 vColor;
layout(location = 0) out highp vec4 fragColor;
void main() {
  vec4 t = texture(uMap, vUv);
  fragColor = vec4(t.rgb * vColor, 1.0);
}
`;

export interface ParticleRendererDeps {
  /** The three.js texture of a path when it is drawable now, else `null`. */
  textureOf(path: string): Texture | null;
}

/** What the draw needs of the view: the projection scale `2 tan(fovY / 2)` and the viewport height in pixels. */
export interface PointView {
  readonly k: number;
  readonly viewportHeight: number;
}

/** The world size of a point sprite of `size` at `distance` (D3D `POINTSCALE_C = 1`, `POINTSIZE_MIN = 1`). */
export function pointWorldSize(size: number, distance: number, view: PointView): number {
  return Math.max(size * view.k, (distance * view.k) / view.viewportHeight);
}

interface Slot {
  readonly mesh: Mesh;
  readonly geometry: InstancedBufferGeometry;
  readonly material: RawShaderMaterial;
  start: number;
}

export interface ParticleRenderStats {
  draws: number;
  particles: number;
}

export class ParticleRenderer {
  readonly scene = new Scene();
  readonly stats: ParticleRenderStats = { draws: 0, particles: 0 };
  private data = new Float32Array(PARTICLE_STRIDE * 2048);
  private buffer: InstancedInterleavedBuffer | null = null;
  private readonly slots = new Map<number, Slot>();
  private readonly corner = new BufferAttribute(QUAD_CORNERS, 2);
  private readonly cornerUv = new BufferAttribute(QUAD_UVS, 2);
  private readonly cornerIndex = new BufferAttribute(QUAD_INDEX, 1);
  private disposed = false;

  constructor(private readonly deps: ParticleRendererDeps) {
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;
  }

  pass(): RenderPass {
    return {
      name: 'particles',
      enabled: true,
      render: (ctx) => {
        if (this.stats.draws > 0) ctx.device.renderLayer(this.scene, ctx.camera.camera, null, true);
      },
    };
  }

  private slotOf(type: number): Slot {
    let s = this.slots.get(type);
    if (s) return s;
    const geometry = new InstancedBufferGeometry();
    geometry.setAttribute('position', this.corner);
    geometry.setAttribute('uv', this.cornerUv);
    geometry.setIndex(this.cornerIndex);
    geometry.instanceCount = 0;
    geometry.boundingSphere = new Sphere(new Vector3(), 1e9);
    const material = new RawShaderMaterial({
      name: `particles-${String(type)}`,
      glslVersion: GLSL3,
      vertexShader: VERTEX,
      fragmentShader: FRAGMENT,
      uniforms: { uMap: { value: null }, uSize: { value: [0.3, 0] } },
      side: DoubleSide,
      transparent: true,
      blending: CustomBlending,
      blendEquation: AddEquation,
      blendSrc: OneFactor,
      blendDst: OneFactor,
      blendSrcAlpha: OneFactor,
      blendDstAlpha: OneFactor,
      depthTest: true,
      depthWrite: false,
    });
    const mesh = new Mesh(geometry, material);
    mesh.frustumCulled = false;
    mesh.matrixAutoUpdate = false;
    mesh.visible = false;
    this.scene.add(mesh);
    s = { mesh, geometry, material, start: -1 };
    this.slots.set(type, s);
    return s;
  }

  /** Fills the instance buffer from the manager and sets up the draws of the frame. */
  render(mng: ParticleMng, view: PointView): void {
    if (this.disposed) return;
    const stats = this.stats;
    stats.draws = 0;
    stats.particles = 0;
    let total = 0;
    mng.forEachType((t) => {
      if (t.n > 0 && this.deps.textureOf(t.spec.texture)) total += t.n;
    });
    for (const s of this.slots.values()) s.mesh.visible = false;
    if (total === 0) return;
    if (total * PARTICLE_STRIDE > this.data.length) {
      let cap = this.data.length / PARTICLE_STRIDE;
      while (cap < total) cap *= 2;
      this.data = new Float32Array(cap * PARTICLE_STRIDE);
      this.buffer = null;
      for (const s of this.slots.values()) {
        // the old instance buffer goes with the geometry, the shared unit quad stays (6b)
        releaseSlotGeometry(s.geometry, this.corner, this.cornerUv, this.cornerIndex);
        s.start = -1;
      }
    }
    if (!this.buffer) {
      const b = new InstancedInterleavedBuffer(this.data, PARTICLE_STRIDE, 1);
      b.setUsage(DynamicDrawUsage);
      this.buffer = b;
    }
    const buffer = this.buffer;
    const data = this.data;
    const color = [0, 0, 0];
    let at = 0;
    mng.forEachType((t) => {
      if (t.n === 0) return;
      const tex = this.deps.textureOf(t.spec.texture);
      if (!tex) return;
      const slot = this.slotOf(t.type);
      const start = at;
      for (let i = 0; i < t.n; i++) {
        const o = at * PARTICLE_STRIDE;
        data[o] = t.pos[i * 3] ?? 0;
        data[o + 1] = t.pos[i * 3 + 1] ?? 0;
        data[o + 2] = -(t.pos[i * 3 + 2] ?? 0);
        t.colorOf(i, color);
        data[o + 3] = color[0] ?? 0;
        data[o + 4] = color[1] ?? 0;
        data[o + 5] = color[2] ?? 0;
        at++;
      }
      if (slot.start !== start) {
        const o = start * PARTICLE_STRIDE;
        slot.geometry.setAttribute('iPos', new InterleavedBufferAttribute(buffer, 3, o));
        slot.geometry.setAttribute('iColor', new InterleavedBufferAttribute(buffer, 3, o + 3));
        slot.start = start;
      }
      slot.geometry.instanceCount = t.n;
      const u = slot.material.uniforms['uMap'];
      if (u) u.value = tex;
      const sz = slot.material.uniforms['uSize'];
      if (sz) sz.value = [t.spec.size * view.k, view.k / view.viewportHeight];
      slot.mesh.visible = true;
      stats.draws++;
    });
    stats.particles = at;
    buffer.clearUpdateRanges();
    buffer.addUpdateRange(0, at * PARTICLE_STRIDE);
    buffer.needsUpdate = true;
  }

  dispose(): void {
    this.disposed = true;
    for (const s of this.slots.values()) {
      s.geometry.dispose();
      s.material.dispose();
    }
    this.slots.clear();
    this.scene.clear();
  }
}
