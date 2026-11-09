// The viewport of the admin map (Plan/25 section 3.2, 3.3, 3.7, 3.9): a centre in world units, a scale in world units per CSS pixel and
// the canvas size. World x is screen x, world z is screen y flipped (north up). Pure functions; the engine feeds them and draws the result.

/** The smallest scale: 0.25 u/px (8x magnified native at MPU 4, Plan/25 section 3.3). */
export const MIN_UPP = 0.25;
/** One wheel notch (and the `+` / `-` keys) change the scale by this factor. */
export const WHEEL_STEP = 1.25;
/** Double click. */
export const DOUBLE_CLICK_STEP = 2;
/** The subscription rectangle is the viewport plus this fraction of its size on each side (Plan/25 section 3.7). */
export const SUBSCRIBE_MARGIN = 0.25;
/** Entities within this many CSS pixels outside the canvas are still drawn (Plan/25 section 3.7 "64 px of margin"). */
export const DRAW_MARGIN_PX = 64;
/** `flyTo` lasts this long (Plan/25 section 3.9). */
export const FLY_MS = 350;

export interface View {
  /** The world point at the centre of the canvas. */
  readonly cx: number;
  readonly cz: number;
  /** World units per CSS pixel. */
  readonly upp: number;
  /** The canvas size in CSS pixels. */
  readonly w: number;
  readonly h: number;
}

/** `[x0, z0, x1, z1]`. */
export type Rect = readonly [number, number, number, number];

export function worldToScreen(v: View, x: number, z: number): { sx: number; sy: number } {
  return { sx: v.w / 2 + (x - v.cx) / v.upp, sy: v.h / 2 - (z - v.cz) / v.upp };
}

export function screenToWorld(v: View, sx: number, sy: number): { x: number; z: number } {
  return { x: v.cx + (sx - v.w / 2) * v.upp, z: v.cz - (sy - v.h / 2) * v.upp };
}

/** The world rectangle the canvas shows, grown by `marginPx` CSS pixels on every side. */
export function viewRect(v: View, marginPx = 0): Rect {
  const hw = (v.w / 2 + marginPx) * v.upp;
  const hh = (v.h / 2 + marginPx) * v.upp;
  return [v.cx - hw, v.cz - hh, v.cx + hw, v.cz + hh];
}

/** The rectangle of a subscription: the viewport plus 25 % of its width and height on each side. */
export function subscriptionRect(v: View): Rect {
  const hw = (v.w / 2) * v.upp * (1 + 2 * SUBSCRIBE_MARGIN);
  const hh = (v.h / 2) * v.upp * (1 + 2 * SUBSCRIBE_MARGIN);
  return [v.cx - hw, v.cz - hh, v.cx + hw, v.cz + hh];
}

/**
 * `r` cut to the world `[0, extent]`: nothing exists outside it, and a smaller rectangle can mean a finer tier (a dungeon of one landscape fits
 * the 1,200 m of tier A even when the canvas is wider than the world). A rectangle that does not touch the world is returned as it is.
 */
export function clampRectToWorld(r: Rect, extent: readonly [number, number]): Rect {
  const c: Rect = [
    Math.max(0, r[0]),
    Math.max(0, r[1]),
    Math.min(extent[0], r[2]),
    Math.min(extent[1], r[3]),
  ];
  return c[2] - c[0] >= 1 && c[3] - c[1] >= 1 ? c : r;
}

export function rectContains(r: Rect, x: number, z: number): boolean {
  return x >= r[0] && x <= r[2] && z >= r[1] && z <= r[3];
}

export function rectsOverlap(a: Rect, b: Rect): boolean {
  return a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1];
}

/** The scale limits of a world: finest 0.25 u/px, coarsest "the whole world fits the canvas". */
export function upLimits(
  extent: readonly [number, number],
  w: number,
  h: number,
): [number, number] {
  const fit = fitUpp(extent, w, h);
  return [MIN_UPP, Math.max(MIN_UPP, fit)];
}

/** The scale at which the whole world fits the canvas (5 % of air around it). */
export function fitUpp(extent: readonly [number, number], w: number, h: number): number {
  if (w <= 0 || h <= 0) return 1;
  return Math.max((extent[0] * 1.05) / w, (extent[1] * 1.05) / h);
}

export function clampUpp(upp: number, limits: readonly [number, number]): number {
  if (!Number.isFinite(upp)) return limits[1];
  return Math.min(limits[1], Math.max(limits[0], upp));
}

/** The view that shows the whole world. */
export function fitView(extent: readonly [number, number], w: number, h: number): View {
  return { cx: extent[0] / 2, cz: extent[1] / 2, upp: fitUpp(extent, w, h), w, h };
}

/** Keeps the centre inside the world (a quarter of the viewport beyond its edge is allowed). */
export function clampCentre(v: View, extent: readonly [number, number]): View {
  const halfW = (v.w / 2) * v.upp;
  const halfH = (v.h / 2) * v.upp;
  const slackX = halfW * 0.5;
  const slackZ = halfH * 0.5;
  const cx = Math.min(extent[0] + slackX, Math.max(-slackX, v.cx));
  const cz = Math.min(extent[1] + slackZ, Math.max(-slackZ, v.cz));
  return cx === v.cx && cz === v.cz ? v : { ...v, cx, cz };
}

/** Zoom by `factor` (> 1 zooms in) around the screen point (sx, sy), which keeps its world position. */
export function zoomAt(
  v: View,
  factor: number,
  sx: number,
  sy: number,
  limits: readonly [number, number],
): View {
  const upp = clampUpp(v.upp / factor, limits);
  if (upp === v.upp) return v;
  const before = screenToWorld(v, sx, sy);
  // the point under the cursor stays under the cursor: cx' = before.x - (sx - w/2) * upp'
  return {
    ...v,
    upp,
    cx: before.x - (sx - v.w / 2) * upp,
    cz: before.z + (sy - v.h / 2) * upp,
  };
}

/** Pans by a pointer drag of (dx, dy) CSS pixels (the map follows the pointer). */
export function panBy(v: View, dx: number, dy: number): View {
  return { ...v, cx: v.cx - dx * v.upp, cz: v.cz + dy * v.upp };
}

/** The scale at which `rect` fills the canvas with some air around it. */
export function uppForRect(
  rect: Rect,
  w: number,
  h: number,
  limits: readonly [number, number],
): number {
  const rw = Math.max(1, rect[2] - rect[0]);
  const rh = Math.max(1, rect[3] - rect[1]);
  return clampUpp(Math.max((rw * 1.2) / Math.max(1, w), (rh * 1.2) / Math.max(1, h)), limits);
}

/** "1:2": the label of the scale as the plan writes it (world units per pixel, two significant digits). */
export function zoomLabel(upp: number): string {
  const n = upp >= 10 ? Math.round(upp) : Math.round(upp * 100) / 100;
  return `1:${String(n)}`;
}

/** Whether two views show the same thing (ignoring the canvas size). */
export function sameView(a: View, b: View): boolean {
  return a.cx === b.cx && a.cz === b.cz && a.upp === b.upp && a.w === b.w && a.h === b.h;
}

// ---- the camera flight ------------------------------------------------------------------------------------------------------------

export interface Fly {
  readonly from: View;
  readonly to: View;
  readonly t0: number;
  readonly ms: number;
}

export function easeInOutCubic(t: number): number {
  const c = Math.min(1, Math.max(0, t));
  return c < 0.5 ? 4 * c * c * c : 1 - (-2 * c + 2) ** 3 / 2;
}

export function startFly(from: View, to: View, now: number, ms = FLY_MS): Fly {
  return { from, to, t0: now, ms };
}

/** The view of a flight at `now`, and whether it is over. The scale is interpolated in the log domain so a long flight zooms evenly. */
export function sampleFly(f: Fly, now: number): { view: View; done: boolean } {
  const t = f.ms <= 0 ? 1 : (now - f.t0) / f.ms;
  if (t >= 1) return { view: f.to, done: true };
  const e = easeInOutCubic(t);
  const upp = Math.exp(Math.log(f.from.upp) * (1 - e) + Math.log(f.to.upp) * e);
  return {
    view: {
      cx: f.from.cx + (f.to.cx - f.from.cx) * e,
      cz: f.from.cz + (f.to.cz - f.from.cz) * e,
      upp,
      w: f.to.w,
      h: f.to.h,
    },
    done: false,
  };
}
