// The geometry of the party windows (`CWndPartyInfo::OnDraw` / `OnLButtonDown`, `CWndPartyQuick::SetActiveMember`, `CWndPartySkill::OnDraw`),
// pure so that the numbers are tested without a window. The info tab uses the vanilla row pitch of 15: the repack's `__PARTY_FIND` build
// draws the rows 18 px apart while the selection highlight (`8 + n * 15`) and the hit test (`(y - 10) / 15`) keep 15, so a click on the
// lower rows selected the wrong member and the eighth row fell out of the 130 px page (FIX, listed in `web/NOTES.md`).

import type { Rect } from '../../layout/geometry';

// ---- the info tab (448 x 336 window, the page is 438 x 130) --------------------------------------------------------------------

export const INFO_ROW_TOP = 10;
export const INFO_ROW_PITCH = 15;
/** The x of the columns: status square, level, "L"/"M", job, name, gauge. */
export const INFO_COL = {
  square: 10,
  level: 30,
  cls: 80,
  job: 100,
  name: 180,
  gauge: 275,
} as const;
export const INFO_GAUGE_W = 110;
/** The selection highlight (`RenderFillRect( CRect( 5, 8 + n * 15, 410, 22 + n * 15 ), 0x60ffff00 )`). */
export const INFO_SELECT_FILL = 0x60ffff00;

/** The y of a row. */
export const infoRowY = (i: number): number => INFO_ROW_TOP + i * INFO_ROW_PITCH;

/** The highlight rect of the selected row. */
export function infoSelectRect(i: number): Rect {
  return { l: 5, t: 8 + i * INFO_ROW_PITCH, r: 410, b: 22 + i * INFO_ROW_PITCH };
}

/** The status square (`CRect( 10, y, 20, y + 10 )`, the colour inside `( 11, y + 1, 19, y + 9 )`). */
export function infoSquare(i: number): { outer: Rect; inner: Rect } {
  const y = infoRowY(i);
  return {
    outer: { l: 10, t: y, r: 20, b: y + 10 },
    inner: { l: 11, t: y + 1, r: 19, b: y + 9 },
  };
}

/** The HP gauge of a row (`rect.SetRect( 275, y + 2, 275 + 110, y + 12 )`). */
export function infoGaugeRect(i: number): Rect {
  const y = infoRowY(i);
  return { l: INFO_COL.gauge, t: y + 2, r: INFO_COL.gauge + INFO_GAUGE_W, b: y + 12 };
}

/** `OnLButtonDown`: the row under a point of the page, `-1` outside `CRect( 10, 10, 365, 10 + size * 15 )`. */
export function infoRowAt(x: number, y: number, size: number): number {
  if (x < 10 || x >= 365 || y < INFO_ROW_TOP || y >= INFO_ROW_TOP + size * INFO_ROW_PITCH)
    return -1;
  return Math.trunc((y - INFO_ROW_TOP) / INFO_ROW_PITCH);
}

/** The status colours (`0xAARRGGBB`). */
export const STATUS_COLOR = {
  ok: 0xff0000ff,
  low: 0xffffff00,
  dead: 0xffff0000,
  away: 0xff878787,
  offline: 0xff000000,
} as const;

/** The text colours of a row: black, the leader green (`0xff1fb72d`). */
export const TEXT_COLOR = 0xff000000;
export const LEADER_COLOR = 0xff1fb72d;

// ---- the skill tab (3 x 3 cells) ---------------------------------------------------------------------------------------------------

/** The page rect of both tabs: the tab control is 438 x 148, the strip takes the last 18 px and the scroll-bar column the last 15 px. */
export const TAB_PAGE_SIZE = { w: 423, h: 130 } as const;

/** The cell rect of skill `n` (0-based) of the 3 x 3 grid (`nWidth = width / 3`, `nHeight = height / 3`). */
export function skillCell(n: number, width: number, height: number): Rect {
  const w = Math.trunc(width / 3);
  const h = Math.trunc(height / 3);
  const col = n % 3;
  const row = Math.trunc(n / 3);
  return { l: col * w + 3, t: row * h + 3, r: col * w + 3 + w, b: row * h + 3 + h };
}

/** `OnLButtonDown` / `OnLButtonDblClk`: the cell index under a point, 0-based (`-1` outside the 3 x 3 grid). */
export function skillAt(x: number, y: number, width: number, height: number): number {
  const w = Math.trunc(width / 3);
  const h = Math.trunc(height / 3);
  if (w <= 0 || h <= 0 || x < 0 || y < 0) return -1;
  const col = Math.trunc(x / w);
  const row = Math.trunc(y / h);
  return col > 2 || row > 2 ? -1 : row * 3 + col;
}

/** The position of the icon and the name in a cell (`( j * nWidth + 3, i * nHeight + 3 )`, `( ... + 35, ... + 6 )`). */
export function skillIconPos(n: number, width: number, height: number): { x: number; y: number } {
  const c = skillCell(n, width, height);
  return { x: c.l, y: c.t };
}
export function skillNamePos(n: number, width: number, height: number): { x: number; y: number } {
  const w = Math.trunc(width / 3);
  const h = Math.trunc(height / 3);
  return { x: (n % 3) * w + 35, y: Math.trunc(n / 3) * h + 6 };
}

// ---- the quick window ------------------------------------------------------------------------------------------------------------

/** `SetActiveMember`: the window height for `n` members (60 + 34 each). */
export const quickHeight = (n: number): number => 60 + 34 * n;

/** The button `WIDC_BUTTON1` moves to `( 114, height - 50, 124, height - 40 )`. */
export function quickButtonRect(height: number): Rect {
  return { l: 114, t: height - 50, r: 124, b: height - 40 };
}

/** The member text is cut at 13 characters with "..." (`strMember.GetLength() > 13`). */
export function quickText(text: string): string {
  return text.length > 13 ? `${text.slice(0, 13)}...` : text;
}

/** The window `y` of the invitation (`CPoint( ( rectRoot.right - width ) / 2, 70 )`). */
export const PARTY_CONFIRM_Y = 70;
