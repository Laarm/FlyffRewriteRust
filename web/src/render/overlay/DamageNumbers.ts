// Floating damage numbers (Plan/41 §19.5; 2DRender.cpp:1952-2180; PL-122). This is the state machine and the layout of the
// original's `CDamageNum`; the combat code (doc 17) decides what is shown and calls `RenderFx.damageNumber`.
//
// One number is a sequence of glyphs of `Sfx/DmgEffect.inc` (28 sprites: digits 0-9 of two colour sets at `digit + 14 *
// style`, the styles 4-7 single glyphs 10-13). Per cosmetic TICK a number goes through five states: the projection at
// the start (0), 8 ticks up at 10 px (1), 10 ticks of shake +-3 px (2), 15 ticks of waiting (3), then it grows and
// fades while it drifts up at 2 px (4); it is removed at age 50 ticks (PL-122: the original aged per tick AND per render).

import type { Vec3LH } from '../types';
import type { Projector } from './anchor';
import type { DrawItem, OverlayAssets } from './types';

export const DMG_ATLAS = 'sfx/dmgeffect.inc';

/** `m_nFrame >= 100` of the original at two increments per frame: 50 ticks. */
export const DMG_MAX_AGE = 50;

export interface DamageNumber {
  readonly pos: Vec3LH;
  readonly value: number;
  /** 0 own player victim, 1 other; 4-7 single glyphs (unused by the server). */
  readonly style: number;
  state: 0 | 1 | 2 | 3 | 4;
  /** The screen y, set by state 0. */
  y: number;
  dy: number;
  cnt: number;
  age: number;
  /** State 0 waits for a projector. */
  ready: boolean;
}

/** The alpha and scale of a number (`Render`): 255 and 0.5 until state 4, then `75 - 2 cnt` and `(3 + 0.05 cnt) / 4`. */
export function damageVisual(n: Pick<DamageNumber, 'state' | 'cnt'>): {
  alpha: number;
  scale: number;
} {
  if (n.state === 4) {
    return { alpha: Math.max(0, 75 - n.cnt * 2), scale: (3 + n.cnt * 0.05) / 4 };
  }
  return { alpha: 255, scale: 2 / 4 };
}

export class DamageNumbers {
  readonly #list: DamageNumber[] = [];
  #tick = 0;

  get count(): number {
    return this.#list.length;
  }

  /** The live numbers (tests, the probe). */
  get numbers(): readonly DamageNumber[] {
    return this.#list;
  }

  /** `AddNumber`: style 0 when the victim is the own player, else 1. */
  add(pos: Vec3LH, value: number, style: number): void {
    this.#list.push({
      pos,
      value: Math.max(0, Math.trunc(value)),
      style,
      state: 0,
      y: 0,
      dy: 0,
      cnt: 0,
      age: 0,
      ready: false,
    });
  }

  clear(): void {
    this.#list.length = 0;
  }

  /**
   * One cosmetic tick (60 Hz): removes the numbers that reached the age limit, then runs `CDamageNum::Process` of the
   * rest. `projector` is the camera of the last frame (state 0 reads the screen y of the point one unit above the hit).
   */
  tick(projector: Projector | null): void {
    this.#tick += 1;
    for (let i = this.#list.length - 1; i >= 0; i -= 1) {
      const n = this.#list[i];
      if (n && n.age >= DMG_MAX_AGE) this.#list.splice(i, 1);
    }
    for (const n of this.#list) this.#process(n, projector);
  }

  #process(n: DamageNumber, projector: Projector | null): void {
    switch (n.state) {
      case 0: {
        const p = projector?.project([n.pos[0], n.pos[1] + 1, n.pos[2]]);
        if (!projector) return; // no camera yet: the number waits at its birth
        n.y = p ? p.y : 0;
        n.dy = 10;
        n.state = 1;
        n.ready = true;
        break;
      }
      case 1:
        if (n.cnt >= 8) {
          n.state = 2;
          n.cnt = 0;
          n.dy = 0;
        }
        break;
      case 2:
        n.dy = this.#tick & 1 ? -3 : 3;
        if (n.cnt >= 10) {
          n.state = 3;
          n.dy = 0;
          n.cnt = 0;
        }
        break;
      case 3:
        if (n.cnt >= 15) {
          n.cnt = 0;
          n.dy = 2;
          n.state = 4;
        }
        break;
      case 4:
        break;
    }
    n.y -= n.dy;
    n.cnt += 1;
    n.age += 1;
  }

  /**
   * The sprites of every number for this frame (`Render`): hidden behind the camera, `x` follows the projection of the
   * hit point, `y` is the stored one. `damageSpace` adds the 10 px thousands gap (`features.damage_space`).
   */
  layout(projector: Projector, assets: OverlayAssets, damageSpace: boolean): DrawItem[] {
    const out: DrawItem[] = [];
    for (const n of this.#list) {
      if (!n.ready) continue;
      const p = projector.project([n.pos[0], n.pos[1] + 1, n.pos[2]]);
      if (!p || !projector.project(n.pos)) continue;
      const { alpha, scale } = damageVisual(n);
      let fx = p.x;
      const glyphW = (index: number): number => assets.sprite(DMG_ATLAS, index)?.w ?? 0;
      if (n.style < 4) {
        const digits = String(n.value);
        const len = digits.length;
        fx -= (scale * glyphW(0) * (1 + 0.5 * (len - 1))) / 2;
        for (let i = 0; i < len; i += 1) {
          const index = digits.charCodeAt(i) - 48 + n.style * 14;
          out.push({
            kind: 'sprite',
            atlas: DMG_ATLAS,
            index,
            x: Math.trunc(fx),
            y: Math.trunc(n.y),
            alpha,
            scale,
          });
          if (damageSpace && (len - (i + 1)) % 3 === 0 && len - (i + 1) !== 0) fx += 10;
          fx += glyphW(index) * 0.5 * scale;
        }
      } else {
        const index = n.style - 4 + 10;
        fx -= (glyphW(index) * 0.5 * scale) / 2;
        out.push({
          kind: 'sprite',
          atlas: DMG_ATLAS,
          index,
          x: Math.trunc(fx),
          y: Math.trunc(n.y),
          alpha,
          scale,
        });
      }
    }
    return out;
  }
}
