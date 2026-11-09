// `CPartsFireDragon` (Plan/41 section 17.1 "Bosses", `ModelGlobal.cpp:497-705`, WP-REND-27): the fire breath of the four Meteonykers. One POOL PER BOSS (FIX of the original's
// single global `g_FireDragon`, which every rendered Meteonyker fed and every rendered Meteonyker updated and drew again), updated once per 60 Hz TICK (FIX, PL-134 / ledger X8: the
// original updated in `Render`, so the breath ran faster at a higher frame rate) and drawn ONCE per frame by `BossFx`.
//
//   create       2 particles per tick while the boss breathes: `vel` from `BossFx`, `scale` = 0.3 x the boss's x scale, `fade` 0.7; at most `MAX_PARTS_FIRE_DRAGON` live per pool (the original's
//                fixed array: a property of the effect, not a budget)
//   step         `pos += vel; scale += 0.05; fade -= 0.004 + xRandomF(0.010)`, `fade` clamped at 0 and the particle removed at 0 (`PARTICLE_DRAGON`, `Process`)
//   emit         a camera-facing quad per particle: the corners `(+-0.5, +-0.5, -0.5) * scale` turned by the inverse view rotation (`g_matInvView`, translation zeroed) and added to
//                `pos` - the z of -0.5 puts the quad half a scale toward the camera (KEEP); white with alpha `fade`, texture `SFX/Texture/Sfx_NpcSp1DiePartbo03.dds`, SRCALPHA / ONE
//
// All in LEFT-HANDED game space; the draw list maps to three.js.

import type { DrawList } from './DrawList';
import type { Randf } from './rng';

/** `MAX_PARTS_FIRE_DRAGON` (`ModelGlobal.h:193`). */
export const MAX_PARTS_FIRE_DRAGON = 256;
/** The texture of the particles (`RestoreDeviceObjects`). */
export const FIRE_DRAGON_TEXTURE = 'sfx/texture/sfx_npcsp1diepartbo03.dds';
/** `m_fFade` of a new particle. */
export const FIRE_FADE_START = 0.7;
/** The per-tick growth of `m_fScale`. */
export const FIRE_SCALE_STEP = 0.05;
/** `m_fFade -= FIRE_FADE_BASE + xRandomF(FIRE_FADE_RANDOM)`. */
export const FIRE_FADE_BASE = 0.004;
export const FIRE_FADE_RANDOM = 0.01;

const f32 = Math.fround;

/** `PARTICLE_DRAGON`. */
export interface DragonParticle {
  x: number;
  y: number;
  z: number;
  vx: number;
  vy: number;
  vz: number;
  scale: number;
  fade: number;
}

/** The corners of `m_vFan`: two triangles `(-,-) (-,+) (+,+)` / `(-,-) (+,+) (+,-)` of the unit square at z = -0.5 with their UV. */
export const FAN: readonly {
  readonly v: readonly [number, number, number];
  readonly uv: readonly [number, number];
}[] = [
  { v: [-0.5, -0.5, -0.5], uv: [0, 1] },
  { v: [-0.5, 0.5, -0.5], uv: [0, 0] },
  { v: [0.5, 0.5, -0.5], uv: [1, 0] },
  { v: [-0.5, -0.5, -0.5], uv: [0, 1] },
  { v: [0.5, 0.5, -0.5], uv: [1, 0] },
  { v: [0.5, -0.5, -0.5], uv: [1, 1] },
];

export class FireDragonPool {
  readonly particles: DragonParticle[] = [];

  /** `CPartsFireDragon::Create`: a free slot or nothing (`false`: the 256 are in use). */
  create(pos: readonly number[], scale: number, vel: readonly number[]): boolean {
    if (this.particles.length >= MAX_PARTS_FIRE_DRAGON) return false;
    this.particles.push({
      x: pos[0] ?? 0,
      y: pos[1] ?? 0,
      z: pos[2] ?? 0,
      vx: vel[0] ?? 0,
      vy: vel[1] ?? 0,
      vz: vel[2] ?? 0,
      scale,
      fade: FIRE_FADE_START,
    });
    return true;
  }

  /**
   * One tick of `Process`: every particle moves, grows and fades; a particle whose fade reached 0 goes. `uniform` is `xRandomF(1)` (the CRT `rand() / RAND_MAX`), drawn once per
   * live particle in slot order.
   */
  step(uniform: () => number): void {
    let w = 0;
    for (const p of this.particles) {
      p.x = f32(p.x + p.vx);
      p.y = f32(p.y + p.vy);
      p.z = f32(p.z + p.vz);
      p.scale = f32(p.scale + FIRE_SCALE_STEP);
      p.fade = f32(p.fade - f32(FIRE_FADE_BASE + f32(FIRE_FADE_RANDOM * uniform())));
      if (p.fade < 0) p.fade = 0;
      if (p.fade === 0) continue;
      this.particles[w++] = p;
    }
    this.particles.length = w;
  }

  get live(): number {
    return this.particles.length;
  }

  clear(): void {
    this.particles.length = 0;
  }

  /**
   * Writes one quad per particle to the draw list (`tex` is the drawable texture id, `-1` draws nothing). `invViewRot` is the 16 numbers of `g_matInvView` (rows are the camera's
   * right, up and forward axes; the translation row is ignored). Returns the quads written.
   */
  emit(list: DrawList, tex: number, invViewRot: ArrayLike<number>): number {
    if (tex < 0) return 0;
    const m = (i: number): number => invViewRot[i] ?? 0;
    let n = 0;
    for (const p of this.particles) {
      const s = p.scale;
      // corner = pos + s * (v . R): the quad is the unit square spanned by the camera's right / up rows, centred half a scale toward the camera along -forward
      list.quad(
        tex,
        true,
        s * m(0),
        s * m(1),
        s * m(2),
        s * m(4),
        s * m(5),
        s * m(6),
        p.x - 0.5 * s * m(8),
        p.y - 0.5 * s * m(9),
        p.z - 0.5 * s * m(10),
        p.fade * 255,
      );
      n++;
    }
    return n;
  }
}

/** `xRandomF(f)` over a `Randf`-style CRT source: `f * rand() / RAND_MAX`. */
export const xRandomF = (uniform: () => number, f: number): number => f32(f * uniform());

export type { Randf };
