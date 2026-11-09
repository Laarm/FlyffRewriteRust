// Icon animations as CSS (Plan/42 §6.3): the cooldown radar, the enchant glow and the drag-highlight pulse run as CSS
// animations keyed by start/end timestamps, never as per-frame JavaScript (§2.4 rule 3).
//
// The cooldown wipe of the original (`RenderRadar`, WndManager.cpp:146-229) draws a white 32 x 32 square at alpha 120
// and, over it, four 15 x 15 quadrant images `Icon_CoolTime_000..127.tga` (alpha 200): every frame `f` of a quadrant
// (0..31) uncovers an equal AREA of it (8 of 256 px per frame, measured from the real files), so the visible hand
// does not turn uniformly. `radarAngleDeg` reproduces that rule and `radarKeyframesCss` bakes it into 128 keyframes
// of a registered `--sweep` angle (one per frame), so the CSS wipe matches the 128-frame rule to within one frame.

import { FRAME_MS } from '../../platform/time';
import type { PixelImage } from '../skin/pixels';

/** Number of cooldown frames and quadrants of `Icon_CoolTime_*.tga`. */
export const RADAR_FRAMES = 128;
/** `Icon_ImgIncAni_00..10`. */
export const ENCHANT_FRAMES = 11;
/** `m_dwHighAlpha += 10` per frame wraps at 256: 26 steps (Plan/42 §6.3). */
export const DRAG_PULSE_FRAMES = 26;

/** `MulDiv(a, b, c)`: `round(a * b / c)` (ties away from zero), `-1` for a zero divisor like the Win32 function. */
export function mulDiv(a: number, b: number, c: number): number {
  if (c === 0) return -1;
  const v = (a * b) / c;
  return v < 0 ? -Math.round(-v) : Math.round(v);
}

/** The frame index `RenderRadar` draws for `elapsed` of `total` (0 = all covered, 127 = all uncovered). */
export function radarIndex(elapsed: number, total: number): number {
  return mulDiv(elapsed, RADAR_FRAMES - 1, total);
}

/**
 * The clockwise hand angle (degrees from 12 o'clock) of radar frame `idx`: the region before it is uncovered, the
 * rest still covered. Frame `f` of quadrant `q` (`idx = 32 q + f`) uncovers an area fraction `(f + 1) / 32` of
 * the quadrant (frame 0 of a quadrant uncovers nothing of it); the hand angle follows from the area of the wedge
 * of a unit square: `atan(2u)` up to the diagonal, `90 - atan(2 (1 - u))` after it.
 */
export function radarAngleDeg(idx: number): number {
  const i = Math.max(0, Math.min(RADAR_FRAMES - 1, Math.trunc(idx)));
  const q = i >> 5;
  const f = i & 31;
  const u = f === 0 ? 0 : (f + 1) / 32;
  const deg = (r: number): number => (r * 180) / Math.PI;
  const inQuadrant = u <= 0.5 ? deg(Math.atan(2 * u)) : 90 - deg(Math.atan(2 * (1 - u)));
  return 90 * q + inQuadrant;
}

/**
 * Number of keyframe segments of the generated CSS animation: one stop per radar frame (128 stops at `k / 127`), so
 * the browser's linear interpolation between stops is never more than one frame away from the frame rule.
 */
export const RADAR_KEYFRAME_SEGMENTS = RADAR_FRAMES - 1;

/** The `--sweep` value of keyframe stop `k` (0..127): the hand angle of radar frame `k`, at progress `k / 127`. */
export function radarStopDeg(k: number): number {
  if (k <= 0) return 0;
  if (k >= RADAR_KEYFRAME_SEGMENTS) return 360;
  return radarAngleDeg(k);
}

/** Linear interpolation between the keyframe stops: what the browser animates at progress `p` (0..1). */
export function cssSweepDeg(p: number): number {
  const x = Math.max(0, Math.min(1, p)) * RADAR_KEYFRAME_SEGMENTS;
  const k = Math.min(RADAR_KEYFRAME_SEGMENTS - 1, Math.floor(x));
  const a = radarStopDeg(k);
  const b = radarStopDeg(k + 1);
  return a + (b - a) * (x - k);
}

/** The `@keyframes flyff-radar` rule (128 stops of the registered angle `--sweep`). */
export function radarKeyframesCss(): string {
  const stops: string[] = [];
  for (let k = 0; k <= RADAR_KEYFRAME_SEGMENTS; k += 1) {
    const pct = ((k * 100) / RADAR_KEYFRAME_SEGMENTS)
      .toFixed(4)
      .replace(/(\.\d*?)0+$/, '$1')
      .replace(/\.$/, '');
    stops.push(`${pct}% { --sweep: ${radarStopDeg(k).toFixed(3)}deg; }`);
  }
  return `@keyframes flyff-radar {\n  ${stops.join('\n  ')}\n}`;
}

/** The stylesheet of the three animations (class names are the contract with the icon cells of WP-UI-12). */
export const ICON_ANIM_CSS = `
@property --sweep { syntax: '<angle>'; inherits: false; initial-value: 0deg; }
${radarKeyframesCss()}
@keyframes flyff-enchant { from { background-position-x: 0; } to { background-position-x: calc(-32px * ${ENCHANT_FRAMES}); } }
@keyframes flyff-pulse { from { opacity: 0; } to { opacity: 1; } }
/* cooldown radar: a white 32x32 square at alpha 120 plus the clock wipe over the still covered part (alpha 200) */
.flyff-cooldown {
  position: absolute; left: 0; top: 1px; width: 32px; height: 32px; pointer-events: none;
  --sweep: 0deg;
  background:
    conic-gradient(from 0deg, transparent 0deg, transparent var(--sweep), rgba(255,255,255,0.784) var(--sweep), rgba(255,255,255,0.784) 360deg),
    rgba(255,255,255,0.47);
  animation-name: flyff-radar; animation-timing-function: linear; animation-iteration-count: 1; animation-fill-mode: both;
}
/* enchant glow: the 11-frame strip (set --flyff-enchant-strip: url(...)) stepped one frame per 60 Hz frame */
.flyff-enchant {
  position: absolute; left: 0; top: 0; width: 32px; height: 32px; pointer-events: none;
  background-image: var(--flyff-enchant-strip); background-size: calc(32px * ${ENCHANT_FRAMES}) 32px; background-repeat: no-repeat;
  animation: flyff-enchant calc(${ENCHANT_FRAMES} * ${FRAME_MS.toFixed(3)}ms) steps(${ENCHANT_FRAMES}) infinite;
}
/* drag highlight of taskbar slots: alpha 0..255 in steps of 10 per frame, wrapping */
.flyff-drag-pulse {
  animation: flyff-pulse calc(${DRAG_PULSE_FRAMES} * ${FRAME_MS.toFixed(3)}ms) linear infinite;
}
/* /dev/ui?e2e=1 freezes every animation for deterministic screenshots */
.flyff-e2e .flyff-enchant, .flyff-e2e .flyff-drag-pulse { animation-play-state: paused; }
`;

/** `style` of a cooldown overlay: the animation runs `total` ms and starts `elapsed` ms in (a negative delay). */
export function cooldownStyle(totalMs: number, elapsedMs: number): Record<string, string> {
  return {
    animationDuration: `${Math.max(1, totalMs)}ms`,
    animationDelay: `${-Math.max(0, elapsedMs)}ms`,
  };
}

const STYLE_ID = 'flyff-ui-icon-anim';

/** Installs {@link ICON_ANIM_CSS} once per document (idempotent). */
export function installIconAnimations(doc: Document = document): void {
  if (doc.getElementById(STYLE_ID)) return;
  const el = doc.createElement('style');
  el.id = STYLE_ID;
  el.textContent = ICON_ANIM_CSS;
  doc.head.appendChild(el);
}

/**
 * Composes the 11 enchant frames (`Icon_ImgIncAni_00..10`, 32 x 32) into one horizontal strip; a missing frame
 * (`undefined`) leaves its cell transparent. The strip is displayed with `steps(11)` (`.flyff-enchant`).
 */
export function composeEnchantStrip(frames: readonly (PixelImage | undefined)[]): PixelImage {
  const cell = 32;
  const width = cell * ENCHANT_FRAMES;
  const data = new Uint8ClampedArray(width * cell * 4);
  for (let f = 0; f < ENCHANT_FRAMES; f += 1) {
    const src = frames[f];
    if (!src) continue;
    const w = Math.min(cell, src.width);
    const h = Math.min(cell, src.height);
    for (let y = 0; y < h; y += 1) {
      for (let x = 0; x < w; x += 1) {
        const si = (y * src.width + x) * 4;
        const di = (y * width + f * cell + x) * 4;
        data[di] = src.data[si] ?? 0;
        data[di + 1] = src.data[si + 1] ?? 0;
        data[di + 2] = src.data[si + 2] ?? 0;
        data[di + 3] = src.data[si + 3] ?? 0;
      }
    }
  }
  return { width, height: cell, data, hasAlpha: true };
}
