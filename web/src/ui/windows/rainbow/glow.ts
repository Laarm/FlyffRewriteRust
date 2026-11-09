// Pure helpers of the progress list and the ladder (`CWndRainbowRaceMiniGame::FillRect`, `CRainbowRace::GetGameKey`; `src:_Interface/WndRainbowRace.cpp:456-486, 537-589`, `_Common/RainbowRace.cpp:98-110`): kept out of the
// window modules (both lazy chunks read them).

import type { RectEntry } from '../../controls/RectLayer';
import type { Rect } from '../../layout/geometry';
import { RMG_MAX } from './constants';

/** The thickness of the pulsing frame (`nthick`). */
export const GLOW_THICK = 8;

/** `CRainbowRace::GetGameKey`: the lowest set bit of the current game, -1 for none. */
export function gameKeyOf(nowGame: number): number {
  for (let i = 0; i < RMG_MAX; i += 1) {
    if (((1 << i) & nowGame) !== 0) return i;
  }
  return -1;
}

/**
 * The four strips of `FillRect( .., bLamp = TRUE )`: each edge fades from the colour (alpha = lerp) at the border to transparent 8 px inside. The CSS gradients stand in for the vertex colours of the
 * original (the colour only changes in alpha).
 */
export function glowStrips(
  key: string,
  rect: Rect,
  rgb: readonly [number, number, number],
  lerp: number,
  thick = GLOW_THICK,
): RectEntry[] {
  const [r, g, b] = rgb;
  const near = `rgba(${String(r)}, ${String(g)}, ${String(b)}, ${String(Math.round(lerp * 1000) / 1000)})`;
  const far = `rgba(${String(r)}, ${String(g)}, ${String(b)}, 0)`;
  const w = rect.r - rect.l;
  const h = rect.b - rect.t;
  const strip = (
    name: string,
    x: number,
    y: number,
    sw: number,
    sh: number,
    to: string,
  ): RectEntry => ({
    key: `${key}-${name}`,
    x,
    y,
    w: sw,
    h: sh,
    fill: `linear-gradient(${to}, ${near}, ${far})`,
  });
  return [
    strip('l', rect.l, rect.t, thick, h, 'to right'),
    strip('t', rect.l, rect.t, w, thick, 'to bottom'),
    strip('r', rect.l + w - thick, rect.t, thick, h, 'to left'),
    strip('b', rect.l, rect.t + h - thick, w, thick, 'to top'),
  ];
}
