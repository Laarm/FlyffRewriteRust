// SFX key interpolation (Plan/41 section 16.3, ref/20 section 4.2; `CSfxPart::GetKey` and the inline copies in the part
// renderers, sfxbase.cpp:107-190).
//
//   prev = the LAST key with frame <= f        (`GetPrevKey`)
//   next = the FIRST key with frame >= f       (`GetNextKey(f, bSkip = TRUE)`, the default of sfxbase.h:117)
//   either one missing: the part is not drawn (it is visible on [first.frame, last.frame] only)
//   d == 0: the values of `prev`; else t = (f - prev.frame) / d and pos, posRotate, rotate, scale are linear in t
//   alpha is INTEGER arithmetic: prev.alpha + (next.alpha - prev.alpha) * (f - prev.frame) / d, C truncation toward zero
//
// The effective alpha is `alpha & 0xFF` (KEEP-QUIRK: `TEXTUREFACTOR = alpha << 24 | 0x404040`, 12 keys of 6 files use 2555 or
// 330 and flicker in the original). The key list is the decoder's `effective_keys()` (sorted, one key per frame).

import type { SfxKey } from '../../gen/formats';

export type V3 = [number, number, number];

/** The interpolated values of one part at one frame. */
export interface SampledKey {
  pos: V3;
  posRotate: V3;
  rotate: V3;
  scale: V3;
  /** Raw integer alpha (not wrapped). */
  alpha: number;
  /** The fraction of the key segment (`(f - prev.frame) / d`, 0 when `d == 0`): the spin of particles reads it. */
  t: number;
}

export function newSampledKey(): SampledKey {
  return {
    pos: [0, 0, 0],
    posRotate: [0, 0, 0],
    rotate: [0, 0, 0],
    scale: [0, 0, 0],
    alpha: 0,
    t: 0,
  };
}

/** The wrapped alpha byte `(alpha << 24) >> 24`: 0..255 whatever the stored value is. */
export const alphaByte = (alpha: number): number => alpha & 0xff;

/** `alphaByte / 255`: the texture-factor alpha as a 0..1 multiplier. */
export const alphaUnit = (alpha: number): number => (alpha & 0xff) / 255;

/**
 * Samples `keys` at frame `f` into `out`. Returns `false` when the part is not visible at `f` (no key at or before / at or
 * after `f`). `interpolateScale = false` keeps the scale of `prev` (particle emitters with `repeat_scale`).
 */
export function sampleKey(
  keys: readonly SfxKey[],
  f: number,
  out: SampledKey,
  interpolateScale = true,
): boolean {
  let prev: SfxKey | undefined;
  let next: SfxKey | undefined;
  for (const k of keys) {
    if (k.frame <= f) prev = k;
    if (k.frame >= f) {
      next = k;
      break;
    }
  }
  if (!prev || !next) return false;
  const d = next.frame - prev.frame;
  copy3(out.pos, prev.pos);
  copy3(out.posRotate, prev.pos_rotate);
  copy3(out.rotate, prev.rotate);
  copy3(out.scale, prev.scale);
  out.alpha = prev.alpha;
  out.t = 0;
  if (d !== 0) {
    const t = (f - prev.frame) / d;
    out.t = t;
    lerp3(out.pos, prev.pos, next.pos, t);
    lerp3(out.posRotate, prev.pos_rotate, next.pos_rotate, t);
    lerp3(out.rotate, prev.rotate, next.rotate, t);
    if (interpolateScale) lerp3(out.scale, prev.scale, next.scale, t);
    out.alpha = prev.alpha + Math.trunc(((next.alpha - prev.alpha) * (f - prev.frame)) / d);
  }
  return true;
}

function copy3(o: V3, a: readonly number[]): void {
  o[0] = a[0] ?? 0;
  o[1] = a[1] ?? 0;
  o[2] = a[2] ?? 0;
}

function lerp3(o: V3, a: readonly number[], b: readonly number[], t: number): void {
  const a0 = a[0] ?? 0;
  const a1 = a[1] ?? 0;
  const a2 = a[2] ?? 0;
  o[0] = a0 + ((b[0] ?? 0) - a0) * t;
  o[1] = a1 + ((b[1] ?? 0) - a1) * t;
  o[2] = a2 + ((b[2] ?? 0) - a2) * t;
}

/** `true` while some key has `frame >= f` (`GetNextKey(f)`): the non-particle parts keep an effect alive with it. */
export function hasKeyAtOrAfter(keys: readonly SfxKey[], f: number): boolean {
  for (let i = keys.length - 1; i >= 0; i--) {
    const k = keys[i];
    if (k && k.frame >= f) return true;
    if (k && k.frame < f) return false;
  }
  return false;
}

/**
 * The per-particle alpha (sfxbase.cpp:2670-2685), integer arithmetic: fading in while `age < appear`
 * (`age * keyAlpha / appear`), fading out while `age > keep` (`keyAlpha - keyAlpha * (age - keep) / (disappear - keep)`), else
 * the key alpha. Returns the raw integer (wrap it with {@link alphaByte}).
 */
export function particleAlpha(
  keyAlpha: number,
  age: number,
  appear: number,
  keep: number,
  disappear: number,
): number {
  if (age < appear) return Math.trunc((age * keyAlpha) / appear);
  if (age > keep) {
    const span = disappear - keep;
    // a particle past `disappear` is gone before it is drawn; a zero span can only be reached by hand-made data
    if (span === 0) return 0;
    return keyAlpha - Math.trunc((keyAlpha * (age - keep)) / span);
  }
  return keyAlpha;
}

/**
 * `GetTextureName(tex, i)` (sfxbase.cpp:21-33, ref/20 section 3.6): the extension is assumed to be exactly 3 characters, the
 * two characters before the dot are the start number `n` (`atoi`), frame `i` is `tex[0 .. len-6] + "%02d" % (n + i) + tex[len-4 ..]`.
 */
export function textureNameOfFrame(tex: string, i: number): string {
  const len = tex.length;
  if (len < 6) return tex;
  const n = Number.parseInt(tex.slice(len - 6, len - 4), 10);
  const start = Number.isNaN(n) ? 0 : n;
  return `${tex.slice(0, len - 6)}${String(start + i).padStart(2, '0')}${tex.slice(len - 4)}`;
}

/** The index of the animation frame of a texture-animated part (`(texFrame * n / texLoop) % texFrame`, integer arithmetic). */
export function textureFrameIndex(texFrame: number, n: number, texLoop: number): number {
  if (texFrame <= 1) return 0;
  if (texLoop <= 0) return 0;
  return Math.trunc((texFrame * n) / texLoop) % texFrame;
}
