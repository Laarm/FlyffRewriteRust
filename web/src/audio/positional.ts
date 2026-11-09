// The legacy positional math of the sound effects (Plan/44 §7.3, WP-AUDIO-05; closed form of `CSoundManager::Play`,
// `src:_DirectX/dsutil.cpp:264-290`). Everything here runs in the LEFT-HANDED world frame and in the legacy listener frame;
// the single place where a vector leaves LH is the `PannerNode` position, which is `mirrorPos` of `render/space.ts`
// (01 §7.3 B8, 44 §2.3): audio has no coordinate conversion of its own.
//
//   v  = S - L                                  source minus listener (the local player)
//   r  = sqrt(v.x^2 + v.z^2)                    r > 100 -> SKIP (no buffer is used)
//   th = rotx * pi / 180                        rotx: the camera's RAW yaw `m_fCurRotx` in degrees
//   px = trunc(v.x cos th - v.z sin th)         = (int)(sin(a - th) * r) of the original acos form
//   pz = trunc(v.z cos th + v.x sin th)         = (int)(cos(a - th) * r)
//   py = trunc(v.y)
//   d  = sqrt(px^2 + py^2 + pz^2)               d > 70 -> MUTED (the voice occupies its slot but is inaudible)
//   gain (by the PannerNode, inverse model, refDistance 4) = d <= 4 ? 1 : 4 / d
//
// The position is computed ONCE at play time and frozen (KEEP-QUIRK: DirectSound buffers kept their play-time position).

import { mirrorPos } from '../render/space';

/** `flMinDistance` of the DirectSound buffers = `refDistance` of the panner (44 §4.1). */
export const SFX_REF_DISTANCE = 4;
/** `flMaxDistance` with `DSBCAPS_MUTE3DATMAXDISTANCE`: muted beyond. */
export const SFX_MUTE_DISTANCE = 70;
/** The horizontal distance beyond which a sound is not played at all (`dsutil.cpp:266-269`). */
export const SFX_CULL_HORIZONTAL = 100;

export type Vec3 = readonly [number, number, number];

/** The outcome of {@link relativePosition}. */
export type Relative =
  | { readonly kind: 'skip' }
  | { readonly kind: 'muted' }
  | {
      readonly kind: 'play';
      /** The buffer position in the legacy listener frame (integers, no `-0`). */
      readonly px: number;
      readonly py: number;
      readonly pz: number;
      /** The `PannerNode` position: `mirrorPos(px, py, pz)` (no `-0`). */
      readonly panner: readonly [number, number, number];
      /** `sqrt(px^2 + py^2 + pz^2)`. */
      readonly distance: number;
    };

const SKIP: Relative = { kind: 'skip' };
const MUTED: Relative = { kind: 'muted' };

/** `x + 0` turns a `-0` into `0` (the goldens are compared with `===`, which already equates them; JSON has no `-0`). */
const nz = (x: number): number => x + 0;

/**
 * The listener-relative position of a source (44 §7.3). `listener` is the local player's LH position, `rotxDeg` the camera
 * yaw in degrees.
 */
export function relativePosition(listener: Vec3, rotxDeg: number, source: Vec3): Relative {
  const vx = source[0] - listener[0];
  const vy = source[1] - listener[1];
  const vz = source[2] - listener[2];
  const r = Math.sqrt(vx * vx + vz * vz);
  if (!(r <= SFX_CULL_HORIZONTAL)) return SKIP; // also NaN
  const th = (rotxDeg * Math.PI) / 180;
  const c = Math.cos(th);
  const s = Math.sin(th);
  const px = nz(Math.trunc(vx * c - vz * s));
  const pz = nz(Math.trunc(vz * c + vx * s));
  const py = nz(Math.trunc(vy));
  const distance = Math.sqrt(px * px + py * py + pz * pz);
  if (distance > SFX_MUTE_DISTANCE) return MUTED;
  const [mx, my, mz] = mirrorPos(px, py, pz);
  return { kind: 'play', px, py, pz, panner: [nz(mx), nz(my), nz(mz)], distance };
}

/** The gain the panner's inverse model gives at distance `d` (the golden table checks it; WebAudio computes it itself). */
export function inverseGain(distance: number): number {
  return distance <= SFX_REF_DISTANCE ? 1 : SFX_REF_DISTANCE / distance;
}
