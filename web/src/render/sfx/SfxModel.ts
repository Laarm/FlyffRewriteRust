// `CSfxModel`: one effect instance (Plan/41 section 16.2, ref/20 section 5; sfxbase.cpp:1885-2975). It owns the frame counter and
// the particle lists; `process()` is exactly the original's per-frame simulation (60 Hz cosmetic step) and returns `true` when the
// effect has finished.
//
//   cur += 1; finished = true
//   for each part i:
//     PARTICLE: not in use -> nothing (a hidden particle layer never keeps the effect alive)
//       every live particle: pos += speed; speed += accel; scale += scale_speed (unless repeat_scale); age += 1; gone at age >= disappear
//       start = keys.first.frame, end = keys.last.frame;  emission window  start <= cur <= end - disappear:
//         r1, r2, r3 = RANDF x 3 (ONE triple per part and frame, shared by this frame's spawns: `__SFX_OPT`)
//         create == 0: spawn `create_num` when cur == start;  else every `create` frames from start, other frames: not finished
//       live particles: not finished;  cur < start: not finished
//       repeat (v0.3): when cur / end >= 0.65 the frame jumps back to int(end * 0.6)
//     any other part: a key at or after `cur` exists: not finished (even for a hidden part)
//
// New particles are not advanced in the frame they spawn. The ping-pong scale of `repeat_scale` particles ran inside the
// original's render function; it runs here in the tick (FIX of the render-rate dependency, PL-134 family).

import type { ParticleParams } from '../../gen/formats';
import { hasKeyAtOrAfter } from './keys';
import type { PartProto, SfxProto } from './proto';
import type { Randf } from './rng';

/** Hard cap of simultaneous particles of ONE part (the fireworks effect peaks at about 9 900). */
export const MAX_PARTICLES_PER_PART = 16384;

const f32 = Math.fround;

/** The live particles of one part, struct of arrays, kept in spawn order (the draw order of the original's list). */
export class ParticleList {
  n = 0;
  #cap = 0;
  pos = new Float32Array(0);
  speed = new Float32Array(0);
  scale = new Float32Array(0);
  scaleSpeed = new Float32Array(0);
  rotation = new Float32Array(0);
  age = new Uint16Array(0);
  /** `bSwScal`: the ping-pong scale is shrinking. */
  shrink = new Uint8Array(0);

  /** Reserves room for one more particle; `false` at the cap. */
  #reserve(): boolean {
    if (this.n >= MAX_PARTICLES_PER_PART) return false;
    if (this.n < this.#cap) return true;
    const cap = Math.min(MAX_PARTICLES_PER_PART, Math.max(16, this.#cap * 2));
    const grow = (a: Float32Array<ArrayBuffer>, per: number): Float32Array<ArrayBuffer> => {
      const b = new Float32Array(cap * per);
      b.set(a);
      return b;
    };
    this.pos = grow(this.pos, 3);
    this.speed = grow(this.speed, 3);
    this.scale = grow(this.scale, 3);
    this.scaleSpeed = grow(this.scaleSpeed, 3);
    this.rotation = grow(this.rotation, 3);
    const age = new Uint16Array(cap);
    age.set(this.age);
    this.age = age;
    const shrink = new Uint8Array(cap);
    shrink.set(this.shrink);
    this.shrink = shrink;
    this.#cap = cap;
    return true;
  }

  /** Appends a particle; returns its index or `-1` at the cap. */
  add(): number {
    if (!this.#reserve()) return -1;
    return this.n++;
  }

  clear(): void {
    this.n = 0;
  }
}

/** The state of one prototype instance. */
export class SfxModel {
  /** `m_vPos` (LH). The runtime adds the `(0, 0.2, 0)` lift before rendering (SfxObj.cpp:241-260). */
  readonly pos: [number, number, number] = [0, 0, 0];
  /** `m_vRotate` in degrees `(x = pitch, y = yaw, z = roll)`: `y` is the object angle of the yaw-only path. */
  readonly rotate: [number, number, number] = [0, 0, 0];
  /** `m_vScale`. */
  readonly scale: [number, number, number] = [1, 1, 1];
  /** `m_nCurFrame`. */
  cur = 0;
  /**
   * The 60 Hz steps this instance has run (never reset by `repeat` or `restart`): the clock of its MESH parts. The original advanced
   * ONE shared mesh clock once per instance per tick (two instances ran the animation twice as fast); here every instance owns its
   * clock (PL-134, `pl_134_sfx_mesh_per_instance`).
   */
  ticks = 0;
  /**
   * `__YOYO_MODEL_FIX`: the mesh of part 0 replaced by another model (`CreateSfxYoYo` points `Part(0)->m_strTex` at the attacker's
   * right-hand weapon, so the flying yo-yo looks like the real one). A VFS path, per instance - the prototype stays shared.
   */
  meshOverride: string | null = null;
  readonly lists: (ParticleList | null)[];

  constructor(
    readonly proto: SfxProto,
    private readonly randf: Randf,
  ) {
    this.lists = proto.parts.map((p) => (p.kind === 'particle' ? new ParticleList() : null));
  }

  /** The model a MESH part draws: its own, or the instance's override for part 0. */
  meshPathOf(part: { readonly index: number; readonly meshPath: string | null }): string | null {
    return part.index === 0 && this.meshOverride !== null ? this.meshOverride : part.meshPath;
  }

  /** `true` if any particle is alive (probe, tests). */
  get liveParticles(): number {
    let n = 0;
    for (const l of this.lists) n += l?.n ?? 0;
    return n;
  }

  /** Static effects loop: back to frame 0 when they finished (Obj.cpp:235-239); the particles in flight stay. */
  restart(): void {
    this.cur = 0;
  }

  /** One 60 Hz step. Returns `true` when the effect has finished. */
  process(): boolean {
    let finished = true;
    this.cur = (this.cur + 1) & 0xffff;
    this.ticks = (this.ticks + 1) >>> 0;
    const parts = this.proto.parts;
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      const list = this.lists[i];
      if (!part) continue;
      if (list) {
        if (!part.inUse) continue;
        const p = part.particle;
        if (!p) continue;
        this.#advance(list, p);
        if (this.#emit(part, list, p)) finished = false;
        if (list.n > 0) finished = false;
        if (this.cur < part.first) finished = false;
        if (p.repeat !== 0) this.#repeat(part);
      } else if (hasKeyAtOrAfter(part.keys, this.cur)) {
        finished = false;
      }
    }
    return finished;
  }

  /** Moves, ages and removes the live particles of a part (the loop at the top of `Process`, stable removal). */
  #advance(list: ParticleList, p: ParticleParams): void {
    const n = list.n;
    const pos = list.pos;
    const speed = list.speed;
    const scale = list.scale;
    const speedScale = list.scaleSpeed;
    const rot = list.rotation;
    const age = list.age;
    const shrink = list.shrink;
    const ax = p.accel[0];
    const ay = p.accel[1];
    const az = p.accel[2];
    const ssx = p.scale_speed[0];
    const ssy = p.scale_speed[1];
    const ssz = p.scale_speed[2];
    const repeatScale = p.repeat_scale !== 0;
    const disappear = p.disappear;
    let w = 0;
    for (let i = 0; i < n; i++) {
      const i3 = i * 3;
      pos[i3] = f32((pos[i3] ?? 0) + (speed[i3] ?? 0));
      pos[i3 + 1] = f32((pos[i3 + 1] ?? 0) + (speed[i3 + 1] ?? 0));
      pos[i3 + 2] = f32((pos[i3 + 2] ?? 0) + (speed[i3 + 2] ?? 0));
      speed[i3] = f32((speed[i3] ?? 0) + ax);
      speed[i3 + 1] = f32((speed[i3 + 1] ?? 0) + ay);
      speed[i3 + 2] = f32((speed[i3 + 2] ?? 0) + az);
      if (!repeatScale) {
        scale[i3] = f32((scale[i3] ?? 0) + ssx);
        scale[i3 + 1] = f32((scale[i3 + 1] ?? 0) + ssy);
        scale[i3 + 2] = f32((scale[i3 + 2] ?? 0) + ssz);
      } else {
        this.#pingPong(list, i, p);
      }
      const a = (age[i] ?? 0) + 1;
      age[i] = a;
      if (a >= disappear) continue; // removed: the stable compaction skips it
      if (w !== i) {
        const w3 = w * 3;
        for (let k = 0; k < 3; k++) {
          pos[w3 + k] = pos[i3 + k] ?? 0;
          speed[w3 + k] = speed[i3 + k] ?? 0;
          scale[w3 + k] = scale[i3 + k] ?? 0;
          speedScale[w3 + k] = speedScale[i3 + k] ?? 0;
          rot[w3 + k] = rot[i3 + k] ?? 0;
        }
        age[w] = age[i] ?? 0;
        shrink[w] = shrink[i] ?? 0;
      }
      w++;
    }
    list.n = w;
  }

  /**
   * The `repeat_scale` ping-pong of the render function, one step: shrinking once `|scale| >= |scale_end|`, growing once
   * `|scale| <= |scale_start|` (`scale_start` is the part's initial scale), `scale -= / += scale_speed(p)`.
   */
  #pingPong(list: ParticleList, i: number, p: ParticleParams): void {
    const i3 = i * 3;
    const sx = list.scale[i3] ?? 0;
    const sy = list.scale[i3 + 1] ?? 0;
    const sz = list.scale[i3 + 2] ?? 0;
    const len = Math.hypot(sx, sy, sz);
    const end = Math.hypot(p.scale_end[0], p.scale_end[1], p.scale_end[2]);
    const start = Math.hypot(p.scale[0], p.scale[1], p.scale[2]);
    let shrink = (list.shrink[i] ?? 0) !== 0;
    if (len >= end) shrink = true;
    else if (len <= start) shrink = false;
    list.shrink[i] = shrink ? 1 : 0;
    const sign = shrink ? -1 : 1;
    list.scale[i3] = f32(sx + sign * (list.scaleSpeed[i3] ?? 0));
    list.scale[i3 + 1] = f32(sy + sign * (list.scaleSpeed[i3 + 1] ?? 0));
    list.scale[i3 + 2] = f32(sz + sign * (list.scaleSpeed[i3 + 2] ?? 0));
  }

  /**
   * The emission window of a part. Returns `true` when the effect is NOT finished because of the spawn interval (a frame inside
   * the window that spawns nothing while `create != 0`).
   */
  #emit(part: PartProto, list: ParticleList, p: ParticleParams): boolean {
    const start = part.first;
    const end = part.last;
    const cur = this.cur;
    if (!(cur >= start && cur <= end - p.disappear)) return false;
    const r = this.randf;
    const r1 = r();
    const r2 = r();
    const r3 = r();
    if (p.create === 0) {
      if (cur === start) this.#spawn(list, p, r1, r2, r3);
      return false;
    }
    if ((cur - start) % p.create === 0) {
      this.#spawn(list, p, r1, r2, r3);
      return false;
    }
    return true;
  }

  /** `create_num` particles with the shared triple `r1 r2 r3` (sfxbase.cpp:2750-2847). */
  #spawn(list: ParticleList, p: ParticleParams, r1: number, r2: number, r3: number): void {
    const randf = this.randf;
    for (let c = 0; c < p.create_num; c++) {
      const j = list.add();
      if (j < 0) return;
      const j3 = j * 3;
      // `ang = RANDF * 360` is handed to sin/cos AS RADIANS (KEEP): effectively a uniform random phase
      const ang = f32(randf() * 360);
      const sa = Math.sin(ang);
      const ca = Math.cos(ang);
      const k = f32(p.xz_low + r2 * (p.xz_high - p.xz_low));
      list.age[j] = 0;
      list.pos[j3] = f32(sa * p.pos_var);
      list.pos[j3 + 1] = f32(r1 * p.pos_var_y);
      list.pos[j3 + 2] = f32(ca * p.pos_var);
      list.speed[j3] = f32(sa * k);
      list.speed[j3 + 1] = f32(p.y_low + r2 * (p.y_high - p.y_low));
      list.speed[j3 + 2] = f32(ca * k);
      // the original copies the part's `m_vScale` (the instance scale enters at draw time)
      list.scale[j3] = p.scale[0];
      list.scale[j3 + 1] = p.scale[1];
      list.scale[j3 + 2] = p.scale[2];
      list.rotation[j3] = f32(p.rotation_low[0] + r1 * (p.rotation_high[0] - p.rotation_low[0]));
      list.rotation[j3 + 1] = f32(
        p.rotation_low[1] + r3 * (p.rotation_high[1] - p.rotation_low[1]),
      );
      list.rotation[j3 + 2] = f32(
        p.rotation_low[2] + r2 * (p.rotation_high[2] - p.rotation_low[2]),
      );
      list.shrink[j] = 0;
      const sr = p.scale_speed_range;
      list.scaleSpeed[j3] = f32(sr[0] + r3 * (sr[1] - sr[0]));
      list.scaleSpeed[j3 + 1] = f32(sr[2] + r2 * (sr[3] - sr[2]));
      list.scaleSpeed[j3 + 2] = f32(sr[4] + r1 * (sr[5] - sr[4]));
    }
  }

  /** `m_bRepeat`: an endless loop in the [60 %, 65 %) region of the last key. */
  #repeat(part: PartProto): void {
    const end = part.last;
    if (end < 0) return;
    const f = f32(this.cur / end);
    if (f >= f32(0.65)) this.cur = Math.trunc(f32(end * f32(0.6)));
  }
}
