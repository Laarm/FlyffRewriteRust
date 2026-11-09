// The audio half of the SFX factory (Plan/44 §9.4 T41/T42, WP-AUDIO-07): every effect the factory creates plays the `sound` of its
// `sfx-table` row at the position it was created at, whether or not the `.sfx` file exists and whatever the SFX render options say
// ("the effect's visibility never gates its sound"). The factory entry of doc 41 is `RenderFx.sfx(xi, options)`; this wrapper puts
// `cues.sfxCreated(xi, posSrc)` in front of it, so every creation path (network `CreateSfxObj`, skill / hit / level-up / item
// effects, shoot effects, yo-yo, item-ready) gets its sound without a call at each site.
//
// A `Proxy` delegates every other member to the wrapped `RenderFx` bound to it, so a method doc 41 adds later keeps working.

import type { RenderFx, SfxOptions } from '../render/fx';
import type { AudioCues } from './cues';
import type { Vec3 } from './deps';

export function withAudioFx<T extends RenderFx>(
  base: T,
  cues: () => Pick<AudioCues, 'sfxCreated'> | null | undefined,
  /** The LH position of a mover (an effect attached to an object has no position of its own). */
  posOf?: (id: number) => Vec3 | undefined,
): T {
  const bound = new Map<PropertyKey, unknown>();
  const sfx = (xi: number, o: SfxOptions): ReturnType<RenderFx['sfx']> => {
    try {
      if (xi > 0) {
        const pos = o.pos ?? (o.src !== undefined && o.src !== 0 ? posOf?.(o.src) : undefined);
        cues()?.sfxCreated(xi, pos);
      }
    } catch {
      // a sound must never stop an effect from being created
    }
    return base.sfx(xi, o);
  };
  return new Proxy(base, {
    get(target, prop) {
      if (prop === 'sfx') return sfx;
      const cached = bound.get(prop);
      if (cached !== undefined) return cached;
      const value: unknown = Reflect.get(target, prop, target);
      if (typeof value !== 'function') return value;
      const fn = (value as (...a: unknown[]) => unknown).bind(target);
      bound.set(prop, fn);
      return fn;
    },
  });
}
