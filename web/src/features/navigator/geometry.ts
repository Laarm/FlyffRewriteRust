// The geometry of the navigator minimap (`CWndNavigator`, `src:_Interface/WndField.cpp:9546-10640`; ref/99 §4.4; Plan/45 §7.6;
// WP-CFEAT-28): the zoom, the pixel scale, the world -> view mapping, the click -> world mapping and the land tiles that cover the
// client rect. All of it is pure: the window feeds the client size and the player position, the painter draws what these functions
// return, the tests check the numbers against the C++ formulas.
//
//   size            pixels of ONE land (`m_size.cx`), default 256, `+-32` per zoom click, clamped to 128..352
//   fx = size / 512 pixels per metre (`MAP_SIZE * MPU = 128 * 4`)
//   xCenter = (int)(px * fx), yCenter = (int)(-pz * fy)   (truncation toward zero)
//   x(obj)  = (int)(W / 2 + obj.x * fx) - xCenter,  y(obj) = (int)(H / 2 - obj.z * fy) - yCenter
//   land (lx, lz) is drawn in row `H_lands - 1 - lz` (north is up), its top-left is (W/2 - xCenter + lx * size, H/2 - yCenter - (lz + 1) * size)

/** `MAP_SIZE * MPU`: the metres of one land edge. */
export const LAND_METERS = 512;
/** The pixel size of one land at the start (`m_size = CSize(256, 256)`). */
export const ZOOM_DEFAULT = 256;
export const ZOOM_MIN = 128;
export const ZOOM_MAX = 352;
export const ZOOM_STEP = 32;

/** The zoom after a click on `+` (`dir` 1) or `-` (`dir` -1), clamped like `OnChildNotify` 100005 / 100006. */
export function stepZoom(size: number, dir: 1 | -1): number {
  return clampZoom(size + dir * ZOOM_STEP);
}

/** `128 <= size <= 352`; anything that is not a finite number falls back to the default. */
export function clampZoom(size: number): number {
  if (!Number.isFinite(size)) return ZOOM_DEFAULT;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.trunc(size)));
}

/** Pixels per metre (`fx` and `fy` are equal: the lands are square). */
export function pixelScale(size: number): number {
  return size / LAND_METERS;
}

export interface ViewSize {
  /** `GetClientRect().Width()` / `Height()` */
  readonly w: number;
  readonly h: number;
}

/** The view centre in pixels: `(int)(px * fx)` and `(int)(-pz * fy)` (C truncation toward zero). */
export function centerOffset(px: number, pz: number, size: number): { xc: number; yc: number } {
  const f = pixelScale(size);
  return { xc: Math.trunc(px * f), yc: Math.trunc(-pz * f) };
}

/** `(LONG)` of the legacy code for the values that appear here (a plain truncation toward zero, `-0` becomes `0`). */
function trunc(v: number): number {
  const t = Math.trunc(v);
  return t === 0 ? 0 : t;
}

/**
 * The view position of a world position for a view centred on `(px, pz)`: `(LONG)(W / 2 + x * fx) - xCenter`,
 * `(LONG)(H / 2 - z * fy) - yCenter` with the integer `W / 2` of `CRect::Width() / 2`.
 */
export function worldToView(
  x: number,
  z: number,
  px: number,
  pz: number,
  size: number,
  view: ViewSize,
): { x: number; y: number } {
  const f = pixelScale(size);
  const { xc, yc } = centerOffset(px, pz, size);
  return {
    x: trunc(Math.trunc(view.w / 2) + x * f) - xc,
    y: trunc(Math.trunc(view.h / 2) - z * f) - yc,
  };
}

/**
 * `OnLButtonDown`: the world position of a click at `(cx, cy)` of the client rect:
 * `pos.x = px + (cx - rect.right / 2) / fx`, `pos.z = pz - (cy - rect.bottom / 2) / fy`.
 */
export function viewToWorld(
  cx: number,
  cy: number,
  px: number,
  pz: number,
  size: number,
  view: ViewSize,
): { x: number; z: number } {
  const f = pixelScale(size);
  return {
    x: px + (cx - Math.trunc(view.w / 2)) / f,
    z: pz - (cy - Math.trunc(view.h / 2)) / f,
  };
}

/**
 * `OnLButtonDblClk` (GM): `vPos.x += pt.x / fx`, `vPos.z -= pt.y / fy` with `pt` relative to the client centre
 * (`rect.Width() / 2`, `rect.Height() / 2`). The same mapping as {@link viewToWorld}; kept as its own name for the chat line.
 */
export function gmTeleportTarget(
  cx: number,
  cy: number,
  px: number,
  pz: number,
  size: number,
  view: ViewSize,
): { x: number; z: number } {
  return viewToWorld(cx, cy, px, pz, size, view);
}

/** One land of the minimap: its coordinates, the draw row of the legacy loop and the pixel position of its top-left. */
export interface TileRect {
  readonly lx: number;
  readonly lz: number;
  /** The row of the legacy loop (`z` of `for z in 0..H`): `landsZ - 1 - lz`. */
  readonly row: number;
  readonly x: number;
  readonly y: number;
  readonly size: number;
}

/** The row of the draw loop that shows land `lz`: `GetLandscape(x, m_nLandHeight - z - 1)` read the other way round. */
export function tileRow(lz: number, landsZ: number): number {
  return landsZ - 1 - lz;
}

/** The land `lz` shown in draw row `row`. */
export function landOfRow(row: number, landsZ: number): number {
  return landsZ - row - 1;
}

/**
 * Every land of the world whose square overlaps the view rect, in the draw order of the legacy loop (rows from the top, lands left
 * to right). A land is drawn with `size x size` pixels.
 *
 * The legacy loop offsets the rows by `nImageBlock * m_nLandWidth` (a bug for a world that is not square; every shipped world is):
 * the row offset here uses the number of rows, which is the same for a square world.
 */
export function visibleTiles(
  lands: readonly [number, number],
  px: number,
  pz: number,
  size: number,
  view: ViewSize,
): TileRect[] {
  const [landsX, landsZ] = lands;
  const { xc, yc } = centerOffset(px, pz, size);
  const ox = Math.trunc(view.w / 2) - xc;
  const oy = Math.trunc(view.h / 2) - yc;
  const out: TileRect[] = [];
  for (let row = 0; row < landsZ; row += 1) {
    const lz = landOfRow(row, landsZ);
    const y = row * size - size * landsZ + oy;
    if (y >= view.h || y + size <= 0) continue;
    for (let lx = 0; lx < landsX; lx += 1) {
      const x = lx * size + ox;
      if (x >= view.w || x + size <= 0) continue;
      out.push({ lx, lz, row, x, y, size });
    }
  }
  return out;
}

/** The mark of `RenderMark`: the top-left of the 32 x 32 sprite scaled by 0.5 whose centre is the marked point. */
export function markPosition(
  markX: number,
  markZ: number,
  px: number,
  pz: number,
  size: number,
  view: ViewSize,
): { x: number; y: number } {
  const f = pixelScale(size);
  // `Pos = player - mark; Pos.x *= fx; Pos.z *= -fy; point = (W / 2 - (Pos.x + 2), H / 2 - (Pos.z + 2))`
  const dx = (px - markX) * f;
  const dz = (pz - markZ) * -f;
  return {
    x: trunc(Math.trunc(view.w / 2) - (dx + 2)),
    y: trunc(Math.trunc(view.h / 2) - (dz + 2)),
  };
}

/** The hit box of a mark (`CRect(point - 8, point + 8)`). */
export const MARK_HIT = 8;
/** Frames of `NavPosition.inc` and the legacy `AccuFrame` interval. */
export const MARK_FRAMES = 4;
export const MARK_FRAME_MS = 120;

/** `m_iFrame` of the mark animation at the time `nowMs` (the legacy counter advances every 120 ms and wraps at 4). */
export function markFrame(nowMs: number): number {
  return Math.floor(nowMs / MARK_FRAME_MS) % MARK_FRAMES;
}

/** The navigator window sizes (`SetWndRect`: `AdjustMinRect( 16 * 6, 16 * 7 )`, `AdjustMaxRect( 16 * 12, 16 * 13 )`). */
export const NAV_MIN_W = 16 * 6;
export const NAV_MIN_H = 16 * 7;
export const NAV_MAX_W = 16 * 12;
export const NAV_MAX_H = 16 * 13;

/**
 * The heading of the player arrow in degrees clockwise from north (+z): the camera looks from `eye` to the player, so the arrow
 * points along `player - eye` (`vDestNor = g_pPlayer->GetPos() - g_Neuz.m_camera.m_vPos`, rotated about the view axis).
 */
export function arrowHeading(eyeX: number, eyeZ: number, px: number, pz: number): number {
  const dx = px - eyeX;
  const dz = pz - eyeZ;
  if (dx === 0 && dz === 0) return 0;
  return (Math.atan2(dx, dz) * 180) / Math.PI;
}
