// The world gauges over the own player's head (Plan/41 section 19.4; MoverRender.cpp:1046-1170 `RenderGauge`, `RenderTurboGauge`).
//
//   charge   (own player, bow / wand charge level 0..4)  `p = (even(trunc(anchor.x - 40) - 2), trunc(anchor.y - 40 - 3) - 2)`:
//            `GauMagChargeBG` then, cumulatively and in THIS order, Tail (level 4), Body2 (>= 3), Body (>= 2), Head (>= 1), all
//            scaled (0.8, 0.55), alpha 200 (the `switch` of the original falls through from 4 down to 1)
//   accel    (casting, collecting and the turbo/fuel bar)  `p = (even(trunc(anchor.x - 40) - 2), trunc(anchor.y - 43) - 2)`:
//            `GauAccelBG` and `GauAccelInner` cropped to `innerW * value / max`, scaled (0.8, 0.55), alpha 200
//
//   turbo    (a flying player, `RenderTurboGauge`, WP-REND-25)  the accel bar at the same place with `value = m_tmAccFuel` of `max = 12000`; with
//            the turbo ON it alternates per TICK between the inner texture and a white fill from `(p.x + 1, p.y + 1)` to
//            `(p.x + trunc(trunc(96 * value / max) * 0.8), p.y + 6)` (the original alternated per rendered frame: render parity, 41 section 28)
//   speed    the flight speed digits `Theme/Theme_FontDigital1.inc` (16 px per digit, then the "km/h" glyph 10) at the projected model
//            origin `(trunc(o.x + 30) even, o.y)`

import { even } from './anchor';
import type { AnchorPx } from './Nameplates';
import type { DrawItem, ImageItem, RectItem, SpriteItem } from './types';
import type { GaugeState } from '../types';

export const CHARGE_BG = 'theme/gaumagchargebg.tga';
export const CHARGE_HEAD = 'theme/gaumagchargehead.tga';
export const CHARGE_BODY = 'theme/gaumagchargebody.tga';
export const CHARGE_BODY2 = 'theme/gaumagchargebody2.tga';
export const CHARGE_TAIL = 'theme/gaumagchargetail.tga';
export const ACCEL_BG = 'theme/gauaccelbg.tga';
export const ACCEL_INNER = 'theme/gauaccelinner.tga';
export const GAUGE_SCALE_X = 0.8;
export const GAUGE_SCALE_Y = 0.55;
export const GAUGE_ALPHA = 200;
/** The digit font of the flight speed: sprites 0-9 are the digits, 10 is "km/h". */
export const DIGITS_ATLAS = 'theme/theme_fontdigital1.inc';
export const KMH_SPRITE = 10;
/** The advance of one digit in pixels. */
export const DIGIT_ADVANCE = 16;
/** `ARGB(0xff, 255, 255, 255)`: the white fill of the blinking turbo bar. */
export const TURBO_BLINK_COLOR = 0xffffffff;

/** `nGaugeWidth = 80`: the half width the gauge is centred with. */
const HALF_WIDTH = 40;

function image(path: string, x: number, y: number, extra: Partial<ImageItem> = {}): ImageItem {
  return {
    kind: 'image',
    path,
    x,
    y,
    alpha: GAUGE_ALPHA,
    scaleX: GAUGE_SCALE_X,
    scaleY: GAUGE_SCALE_Y,
    ...extra,
  };
}

/** The charge gauge of a bow or wand: the background and the filled segments of `level` (1..4); nothing for 0. */
export function layoutChargeGauge(anchor: AnchorPx, level: number): DrawItem[] {
  if (level < 1) return [];
  const x = even(Math.trunc(anchor.x - HALF_WIDTH) - 2);
  const y = Math.trunc(anchor.y - 40 - 3) - 2;
  const items: DrawItem[] = [image(CHARGE_BG, x, y)];
  if (level >= 4) items.push(image(CHARGE_TAIL, x, y));
  if (level >= 3) items.push(image(CHARGE_BODY2, x, y));
  if (level >= 2) items.push(image(CHARGE_BODY, x, y));
  items.push(image(CHARGE_HEAD, x, y));
  return items;
}

/**
 * A fill gauge (`value` of `max`) in the shape of the turbo bar: background and the inner texture cropped to the fraction.
 * `innerW` is the width of the inner texture (known once it loaded; the crop is `trunc(innerW * value / max)`).
 */
export function layoutAccelGauge(
  anchor: AnchorPx,
  value: number,
  max: number,
  innerW: number,
): DrawItem[] {
  if (!(max > 0)) return [];
  const x = even(Math.trunc(anchor.x - HALF_WIDTH) - 2);
  const y = Math.trunc(anchor.y - 43) - 2;
  const v = Math.max(0, Math.min(value, max));
  return [
    image(ACCEL_BG, x, y),
    image(ACCEL_INNER, x, y, { cropW: Math.trunc((innerW * v) / max) }),
  ];
}

/**
 * `CMover::RenderTurboGauge` bar (the frame and the fill): `value` of `max`, with the turbo on the bar alternates per `tick` between
 * the inner texture (odd ticks) and a white fill (even ticks). The widths are the original's integer arithmetic.
 */
export function layoutTurboGauge(
  anchor: AnchorPx,
  value: number,
  max: number,
  innerW: number,
  turbo: boolean,
  tick: number,
): DrawItem[] {
  if (!(max > 0)) return [];
  const x = even(Math.trunc(anchor.x - HALF_WIDTH) - 2);
  const y = Math.trunc(anchor.y - 40 - 3) - 2;
  const v = Math.max(0, Math.min(Math.trunc(value), max));
  const items: DrawItem[] = [image(ACCEL_BG, x, y)];
  const fill = image(ACCEL_INNER, x, y, { cropW: Math.trunc((innerW * v) / max) });
  if (turbo && (tick & 1) === 0) {
    // the white fill: `RenderFillRect(CRect(p.x + 1, p.y + 1, p.x + nWidthScale, p.y + 6))`
    const scaled = Math.trunc(Math.trunc((96 * v) / max) * GAUGE_SCALE_X);
    const rect: RectItem = {
      kind: 'rect',
      x: x + 1,
      y: y + 1,
      w: Math.max(0, scaled - 1),
      h: 5,
      color: TURBO_BLINK_COLOR,
      filled: true,
    };
    items.push(rect);
  } else {
    items.push(fill);
  }
  return items;
}

/** The decimal digits of the flight speed and the "km/h" glyph (`foot` = the projected model origin). */
export function layoutFlightSpeed(foot: AnchorPx, speed: number): SpriteItem[] {
  const digits = String(Math.max(0, Math.trunc(speed)));
  let x = even(Math.trunc(foot.x + 30));
  const y = Math.trunc(foot.y);
  const out: SpriteItem[] = [];
  for (const ch of digits) {
    out.push({
      kind: 'sprite',
      atlas: DIGITS_ATLAS,
      index: ch.charCodeAt(0) - 48,
      x,
      y,
      alpha: 255,
    });
    x += DIGIT_ADVANCE;
  }
  out.push({ kind: 'sprite', atlas: DIGITS_ATLAS, index: KMH_SPRITE, x, y, alpha: 255 });
  return out;
}

/**
 * Everything the own player's gauges draw this frame: the charge gauge, then casting and collecting, then the turbo bar with the flight
 * speed. `tick` is the 60 Hz cosmetic tick counter (the turbo blink), `foot` the projected model origin of the player.
 */
export function layoutGauges(
  anchor: AnchorPx,
  g: GaugeState,
  innerW: number,
  tick = 0,
  foot?: AnchorPx,
): DrawItem[] {
  const out: DrawItem[] = [...layoutChargeGauge(anchor, g.charge)];
  for (const pair of [g.casting, g.collecting]) {
    if (pair) out.push(...layoutAccelGauge(anchor, pair[0], pair[1], innerW));
  }
  if (g.fuel) {
    out.push(...layoutTurboGauge(anchor, g.fuel[0], g.fuel[1], innerW, g.turbo === true, tick));
    if (g.speed !== undefined && foot) out.push(...layoutFlightSpeed(foot, g.speed));
  }
  return out;
}
