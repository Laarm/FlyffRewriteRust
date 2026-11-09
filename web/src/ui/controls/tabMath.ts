// The geometry of the tab control (`CWndTabCtrl`, WndControl.cpp:4219-4795; Plan/42 §7.6, ref/22 §6.5): where the selected
// tab is drawn over the baked strip, where the captions sit and which tab a click hits. Pure functions so that the
// goldens run without a window. The original uses THREE slightly different widths for these (KEEP-QUIRK):
//  * strip and selected tab: `floor((w - 15) / 8)` columns of 8 px split into `n` tabs of `floor(cols / n)` columns;
//  * captions: `floor((w - 15) / n)` px per tab;
//  * clicks: `floor(w / n)` px per tab (`m_nTabButtonLength`).

/** The height of the strip below the pages (pages are `h - 18` high). */
export const TAB_STRIP_HEIGHT = 18;
/** The scroll-bar column the panel and the strip leave free on the right. */
export const TAB_RIGHT_MARGIN = 15;
/** One strip tile (`WndTabTile*.bmp`) is 8 px wide. */
export const TAB_TILE_W = 8;
/** and 20 px high. */
export const TAB_TILE_H = 20;

/** One tile of the selected tab: `tile` is the index 0-5 into `WndTabTile10-15.bmp`, `w` its (stretched) width. */
export interface SelectedTabTile {
  tile: number;
  x: number;
  y: number;
  w: number;
}

/** Top of the strip: `(floor(h / 8) - 2) * 8`. */
export function tabStripTop(h: number): number {
  return (Math.floor(h / 8) - 2) * 8;
}

/**
 * The tiles drawn for the selected tab (`CWndTabCtrl::OnDraw`): the first tile, the middle one stretched `size - 3`
 * times, the end part and the separators (the last tab closes with tile 5). Positions are from the control's
 * top-left. Nothing is drawn for a selection below 0 or a tab narrower than three columns.
 */
export function selectedTabTiles(
  w: number,
  h: number,
  tabs: number,
  cur: number,
): SelectedTabTile[] {
  if (cur < 0) return [];
  const n = Math.max(1, tabs);
  const cols = Math.floor((w - TAB_RIGHT_MARGIN) / TAB_TILE_W);
  const size = Math.floor(cols / n);
  const y = tabStripTop(h);
  const x0 = TAB_TILE_W * size * cur;
  const out: SelectedTabTile[] = [{ tile: cur === 0 ? 0 : 1, x: x0, y, w: TAB_TILE_W }];
  if (size - 3 > 0) out.push({ tile: 1, x: x0 + TAB_TILE_W, y, w: TAB_TILE_W * (size - 3) });
  out.push({ tile: 2, x: x0 + TAB_TILE_W * (size - 2), y, w: TAB_TILE_W });
  if (cur === n - 1) {
    out.push({ tile: 5, x: x0 + TAB_TILE_W * (size - 1), y, w: TAB_TILE_W });
  } else {
    out.push({ tile: 3, x: x0 + TAB_TILE_W * (size - 1), y, w: TAB_TILE_W });
    out.push({ tile: 4, x: x0 + TAB_TILE_W * size, y, w: TAB_TILE_W });
  }
  return out;
}

/** `TabTitleAlign` (`ALIGN_LEFT`, `ALIGN_CENTER`, `ALIGN_RIGHT`). */
export type TabAlign = 'left' | 'center' | 'right';

/** The x of the caption of tab `i` (`OnDraw`): left `i*len + 2`; centre / right by the text width when it fits. */
export function tabCaptionX(
  i: number,
  len: number,
  textW: number,
  align: TabAlign = 'center',
): number {
  const base = i * len;
  if (align === 'left' || textW >= len) return base + 2;
  if (align === 'right') return base + (len - textW);
  return base + Math.trunc((len - textW) / 2);
}

/** The caption's y: `windowBottom - 17`. */
export function tabCaptionY(h: number): number {
  return h - 17;
}

/** The width of a tab for the captions: `floor((w - 15) / n)`. */
export function tabCaptionLen(w: number, tabs: number): number {
  return Math.floor((w - TAB_RIGHT_MARGIN) / Math.max(1, tabs));
}

/** `m_nTabButtonLength`: the width of a tab for clicks, `floor(clientW / n)`. */
export function tabClickLen(clientW: number, tabs: number): number {
  return tabs === 0 ? 0 : Math.floor(clientW / tabs);
}

/** The tab hit by a click at `(x, y)` of the client (`-1` for none): the bottom 18 px, `floor(x / len)`. */
export function tabAt(
  x: number,
  y: number,
  clientW: number,
  clientH: number,
  tabs: number,
): number {
  const len = tabClickLen(clientW, tabs);
  if (len === 0) return -1;
  if (y < clientH - TAB_STRIP_HEIGHT || y >= clientH) return -1;
  const i = Math.floor(x / len);
  return x >= 0 && i < tabs ? i : -1;
}
