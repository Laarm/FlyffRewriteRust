// The skin baker (Plan/42 §7): every frame gets ONE canvas that holds its 12-tile (or single-image) skin, the 9-tile
// skins of its controls and the tab panels, composited with the original's overwrite rule (`AdjustWndBase`,
// WndBase.cpp:2624-2665; `AdditionalSkinTexture`, WndNeuz.cpp:255-335, WndBase.cpp:2524-2619, WndControl.cpp:4428-4590).
//
// The module has three layers:
//  * pure index functions (`frameTileIndex`, `controlTileIndex`, `tileSetNames`, `tabStripTiles`) that carry the
//    geometry rules and are tested against goldens;
//  * `SkinOp`s: a frame (or control) describes WHAT to paint (`frameSkinOps`, `controlSkinOp`);
//  * `SkinBaker`: loads the images of the ops and paints them into a pixel buffer (`bakeNow`).

import {
  WBS_CAPTION,
  WBS_NODRAWFRAME,
  WBS_VSCROLL,
  WSS_GROUPBOX,
  WTYPE_BUTTON,
  WTYPE_COMBOBOX,
  WTYPE_EDITCTRL,
  WTYPE_LISTBOX,
  WTYPE_LISTCTRL,
  WTYPE_STATIC,
  WTYPE_TABCTRL,
  WTYPE_TEXT,
  WTYPE_TREECTRL,
} from '../layout/styles';
import { roundUp16, type Rect, rectHeight, rectWidth } from '../layout/geometry';
import { type KeyMode, MISSING, normalizePath, themePath, type UiImageCache } from './images';
import { blit, createBuffer, fillRect, type PixelBuffer, type PixelImage } from './pixels';

/** A frame skin cell (`WndTile??.tga`) is 16 x 16. */
export const FRAME_TILE = 16;
/** Control skin tiles are 8 x 10 images stepped on an 8 x 8 grid (later rows overwrite the bottom 2 px). */
export const CONTROL_TILE_STEP = 8;
/** `DEF_CTRL_TEXT`: the default skin of Text, Edit, ListBox, ComboBox and Tree controls. */
export const DEF_CTRL_TEXT = 'WndEditTile00.tga';
/** `DEF_CTRL_GROUP`: the default skin of group-box statics. */
export const DEF_CTRL_GROUP = 'WndEditTile200.tga';
/** The tab strip tile set (`WndTabTile00-05.bmp` unselected, `10-15` selected; 8 x 20 each). */
export const TAB_TILE_SET = 'WndTabTile00.bmp';
/** `VSCROLL_WIDTH` taken from a skin rect of a control with `WBS_VSCROLL` (WndBase.cpp:2574). */
export const SKIN_VSCROLL_WIDTH = 16;
/** Right margin of a tab control panel and strip (the scroll-bar column). */
export const TAB_RIGHT_MARGIN = 15;

// ---- names and indices --------------------------------------------------------------------------------------

/**
 * The tile file names of a set (`prefix + %02d + ext`, ref/22 §6.3): `prefix = T[0 .. len-6]`, `ext = T[len-4 ..]`.
 * KEEP: the two digits of the texture name are ignored, so `WndTile07.tga` -> `WndTile00 … WndTile11`,
 * `WndTile209.tga` -> `WndTile200 … WndTile211`, `WndTile308.tga` -> `WndTile300 … WndTile311`.
 */
export function tileSetNames(texture: string, count: number, first = 0): string[] {
  const prefix = texture.slice(0, Math.max(0, texture.length - 6));
  const ext = texture.slice(Math.max(0, texture.length - 4));
  const out: string[] = [];
  for (let i = first; i < first + count; i += 1) {
    out.push(`${prefix}${String(i).padStart(2, '0')}${ext}`);
  }
  return out;
}

/**
 * Tile index of cell `(x, y)` of a 12-tile frame (Plan/42 §7.1; WndNeuz.cpp:296-310). The checks run in the
 * original's order, so a one-row window uses tiles 0-2 and a two-row window never reaches 9-11.
 */
export function frameTileIndex(x: number, y: number, cols: number, rows: number): number {
  const last = x === cols - 1;
  if (y === 0) return x === 0 ? 0 : last ? 2 : 1;
  if (y === 1) return x === 0 ? 3 : last ? 5 : 4;
  if (y < rows - 1) return x === 0 ? 6 : last ? 8 : 7;
  // y === rows - 1
  return x === 0 ? 9 : last ? 11 : 10;
}

/** Tile index of cell `(i, j)` (row, column) of a 9-tile control skin (Plan/42 §7.2; WndBase.cpp:2576-2617). */
export function controlTileIndex(i: number, j: number, rows: number, cols: number): number {
  const last = j === cols - 1;
  if (i === 0) return j === 0 ? 0 : last ? 2 : 1;
  if (i === rows - 1) return j === 0 ? 6 : last ? 8 : 7;
  return j === 0 ? 3 : last ? 5 : 4;
}

/** The 12-tile matrix of a frame (rows of tile indices), for tests and the gallery. */
export function frameTileMatrix(width: number, height: number): number[][] {
  const cols = Math.floor(width / FRAME_TILE);
  const rows = Math.floor(height / FRAME_TILE);
  const out: number[][] = [];
  for (let y = 0; y < rows; y += 1) {
    const row: number[] = [];
    for (let x = 0; x < cols; x += 1) row.push(frameTileIndex(x, y, cols, rows));
    out.push(row);
  }
  return out;
}

/** One cell of the 9-tile grid of a control skin: tile index and pixel offset from the skin rect's top-left. */
export interface Tile9Cell {
  tile: number;
  x: number;
  y: number;
}

/** The cells painted for a skin rect of `w x h` (`cols = floor(w/8)`, `rows = floor(h/8)`: remainders are not covered). */
export function tile9Cells(w: number, h: number): Tile9Cell[] {
  const cols = Math.floor(w / CONTROL_TILE_STEP);
  const rows = Math.floor(h / CONTROL_TILE_STEP);
  const out: Tile9Cell[] = [];
  for (let i = 0; i < rows; i += 1) {
    for (let j = 0; j < cols; j += 1) {
      out.push({
        tile: controlTileIndex(i, j, rows, cols),
        x: j * CONTROL_TILE_STEP,
        y: i * CONTROL_TILE_STEP,
      });
    }
  }
  return out;
}

/** A tab strip tile placed at `x` columns (8 px each) from the strip's left edge (`WndTabTile<index>.bmp`). */
export interface TabStripTile {
  tile: number;
  col: number;
}

/**
 * The tiles of the unselected tab strip (Plan/42 §7.6; WndControl.cpp:4552-4590): `widthCols` 8-px columns
 * (`floor((w - 15) / 8)`) split into `tabs` equal parts of `floor(widthCols / tabs)` columns: tile 0 starts the
 * first tab, 4 starts the next ones, 1 is the middle, 2 and 3 end a tab and separate it from the next one, 5
 * closes the strip when the separator lands on the last column.
 */
export function tabStripTiles(widthCols: number, tabs: number): TabStripTile[] {
  const size = Math.floor(widthCols / Math.max(1, tabs));
  const out: TabStripTile[] = [];
  let cur = 0;
  for (let i = 0; i < widthCols; i += 1) {
    if (i === cur - 2) {
      out.push({ tile: 2, col: i });
      i += 1;
      out.push({ tile: i === widthCols - 1 ? 5 : 3, col: i });
    } else if (i === cur) {
      out.push({ tile: i === 0 ? 0 : 4, col: i });
      cur += size;
    } else {
      out.push({ tile: 1, col: i });
    }
  }
  return out;
}

// ---- skin operations ----------------------------------------------------------------------------------------

/** What to paint into the frame buffer; coordinates are in FRAME-WINDOW pixels. */
export type SkinOp =
  /** The 12-tile frame skin of `width x height` (already a multiple of 16). */
  | { op: 'frame'; texture: string; width: number; height: number }
  /** One image painted at `(x, y)` (non-tiled frame background, a `tile = false` control image). */
  | { op: 'image'; texture: string; x: number; y: number }
  /** A 9-tile control skin over `rect` (the `WBS_VSCROLL` reduction is already applied). */
  | { op: 'tile9'; texture: string; rect: Rect }
  /** A tab control: the `WndEditTile00` panel and the unselected tab strip over `rect`. */
  | { op: 'tabs'; rect: Rect; tabs: number }
  /** The theme fallback of a frame without texture (§7.5). */
  | { op: 'untextured'; width: number; height: number; caption: boolean };

/** The images (path under `theme/` or a full path) one op needs, with the colour-key mode of Plan/42 §6.1. */
export function opImages(op: SkinOp): { path: string; mode: KeyMode }[] {
  switch (op.op) {
    case 'frame':
      return tileSetNames(op.texture, 12).map((t) => ({ path: themePath(t), mode: 'nibble' }));
    case 'image':
      return [{ path: themePath(op.texture), mode: 'nibble' }];
    case 'tile9':
      return tileSetNames(op.texture, 9).map((t) => ({ path: themePath(t), mode: 'nibble' }));
    case 'tabs':
      return [
        ...tileSetNames(DEF_CTRL_TEXT, 9).map((t) => ({
          path: themePath(t),
          mode: 'nibble' as const,
        })),
        ...tileSetNames(TAB_TILE_SET, 6).map((t) => ({
          path: themePath(t),
          mode: 'nibble' as const,
        })),
      ];
    case 'untextured':
      return [{ path: themePath('WindField.bmp'), mode: 'exact' }];
  }
}

/**
 * The op of a layout control (`CWndBase::AdditionalSkinTexture`; Plan/42 §7.2). Buttons and custom placeholders
 * paint nothing (buttons draw their own sprite); statics have no skin unless they are group boxes or the layout
 * gives a texture; Text/Edit/ListBox/ComboBox/Tree default to `WndEditTile00.tga`. The layout's `tile` flag is
 * always the control's (KEEP: a `tile = false` Text control paints the single corner tile image at its top-left).
 *
 * @param rectInFrame the control rect in frame-window coordinates (the client offset already added)
 * @param tabs        the number of tabs of a tab control (the live control passes it; layouts alone use 1)
 */
export function controlSkinOp(
  kind: number,
  style: number,
  texture: string,
  tile: boolean,
  rectInFrame: Rect,
  tabs = 1,
): SkinOp | null {
  if (kind === WTYPE_TABCTRL) return { op: 'tabs', rect: rectInFrame, tabs };
  let tex = texture;
  switch (kind) {
    case WTYPE_STATIC:
      if (tex === '' && (style & WSS_GROUPBOX) !== 0) tex = DEF_CTRL_GROUP;
      break;
    case WTYPE_TEXT:
    case WTYPE_EDITCTRL:
    case WTYPE_LISTBOX:
    case WTYPE_LISTCTRL:
    case WTYPE_COMBOBOX:
    case WTYPE_TREECTRL:
      if (tex === '') tex = DEF_CTRL_TEXT;
      break;
    case WTYPE_BUTTON:
    default:
      return null;
  }
  if (tex === '') return null;
  if (!tile) return { op: 'image', texture: tex, x: rectInFrame.l, y: rectInFrame.t };
  const rect =
    (style & WBS_VSCROLL) !== 0
      ? { ...rectInFrame, r: rectInFrame.r - SKIN_VSCROLL_WIDTH }
      : rectInFrame;
  return { op: 'tile9', texture: tex, rect };
}

/** What `frameSkinOps` needs to know about a frame (`CWndNeuz::AdjustWndBase`, WndNeuz.cpp:241-253). */
export interface FrameSkinInfo {
  style: number;
  texture: string;
  tile: boolean;
  /** The window size (a tiled frame is rounded up to a multiple of 16 here). */
  width: number;
  height: number;
  visible: boolean;
}

/**
 * The ops of the frame itself, or `null` when nothing is baked (`AdjustWndBase` returns early for an invisible
 * frame or `WBS_NODRAWFRAME`). The caller appends the child ops when `childrenBaked` is true: frames with a
 * single-image background do NOT bake child skins (they are part of the picture, ref/22 §6.3).
 */
export function frameSkinOps(
  info: FrameSkinInfo,
): { ops: SkinOp[]; childrenBaked: boolean } | null {
  if (!info.visible || (info.style & WBS_NODRAWFRAME) !== 0) return null;
  if (info.texture === '') {
    return {
      ops: [
        {
          op: 'untextured',
          width: info.width,
          height: info.height,
          caption: (info.style & WBS_CAPTION) !== 0,
        },
      ],
      childrenBaked: false,
    };
  }
  if (!info.tile) {
    return { ops: [{ op: 'image', texture: info.texture, x: 0, y: 0 }], childrenBaked: false };
  }
  return {
    ops: [
      {
        op: 'frame',
        texture: info.texture,
        width: roundUp16(info.width),
        height: roundUp16(info.height),
      },
    ],
    childrenBaked: true,
  };
}

// ---- painting -------------------------------------------------------------------------------------------------

/** What an `afterSkin` hook can do (port of the `AdditionalSkinTexture`/`AfterSkinTexture` overrides). */
export interface SkinPaintContext {
  readonly buffer: PixelBuffer;
  /** A loaded image, `MISSING` or `undefined` (not requested through `afterSkinImages`). */
  image(path: string, mode?: KeyMode): PixelImage | typeof MISSING | undefined;
  /** Paints a loaded image with the overwrite rule (nothing for a missing one). */
  blit(path: string, x: number, y: number, mode?: KeyMode): void;
  /** Paints a 9-tile skin of `texture` over `rect`. */
  tile9(texture: string, rect: Rect): void;
}

/** One baked frame skin: straight-alpha RGBA, ready for `putImageData`. */
export interface BakedSkin extends PixelBuffer {
  /** Paths of the images that were missing (nothing was painted for them; KQ-30). */
  missing: readonly string[];
}

export interface BakeSpec {
  /** The frame window size (tiled frames: already rounded up to 16). */
  width: number;
  height: number;
  ops: readonly SkinOp[];
  /** The extra images the hook needs (preloaded with the ops). */
  afterImages?: readonly { path: string; mode: KeyMode }[];
  after?: (ctx: SkinPaintContext) => void;
}

/** Gradient helper of the untextured title bar (`GradationRect`, ITheme.cpp:973-982). */
function verticalGradient(
  buf: PixelBuffer,
  l: number,
  t: number,
  r: number,
  b: number,
  top: readonly [number, number, number],
  bottom: readonly [number, number, number],
): void {
  const h = b - t;
  for (let y = t; y < b; y += 1) {
    const f = h <= 1 ? 0 : (y - t) / (h - 1);
    fillRect(buf, l, y, r, y + 1, [
      Math.round(top[0] + (bottom[0] - top[0]) * f),
      Math.round(top[1] + (bottom[1] - top[1]) * f),
      Math.round(top[2] + (bottom[2] - top[2]) * f),
      255,
    ]);
  }
}

/** A rounded-rectangle outline (corner pixels cut) or a plain one; 1 px. */
function outline(
  buf: PixelBuffer,
  r: Rect,
  rgb: readonly [number, number, number],
  rounded: boolean,
): void {
  const c: [number, number, number, number] = [rgb[0], rgb[1], rgb[2], 255];
  const cut = rounded ? 1 : 0;
  fillRect(buf, r.l + cut, r.t, r.r - cut, r.t + 1, c);
  fillRect(buf, r.l + cut, r.b - 1, r.r - cut, r.b, c);
  fillRect(buf, r.l, r.t + cut, r.l + 1, r.b - cut, c);
  fillRect(buf, r.r - 1, r.t + cut, r.r, r.b - cut, c);
}

/** Tiles `img` over `rect`, anchored at the rect's top-left (the wrap addressing of `RenderWndBaseFrame`). */
function tileFill(buf: PixelBuffer, img: PixelImage, rect: Rect): void {
  for (let y = rect.t; y < rect.b; y += img.height) {
    for (let x = rect.l; x < rect.r; x += img.width) {
      const w = Math.min(img.width, rect.r - x);
      const h = Math.min(img.height, rect.b - y);
      blit(buf, img, x, y, [0, 0, w, h]);
    }
  }
}

/**
 * Loads the images of ops and paints them. One baker per `UiImageCache`; `bake` is the normal path, `bakeNow`
 * is the synchronous part (all images must be cached already, a missing one paints nothing).
 */
export class SkinBaker {
  readonly images: UiImageCache;

  constructor(images: UiImageCache) {
    this.images = images;
  }

  /** Every image a spec needs, de-duplicated. */
  imagesFor(spec: BakeSpec): { path: string; mode: KeyMode }[] {
    const seen = new Set<string>();
    const out: { path: string; mode: KeyMode }[] = [];
    const add = (i: { path: string; mode: KeyMode }): void => {
      const k = `${i.mode}:${normalizePath(i.path)}`;
      if (seen.has(k)) return;
      seen.add(k);
      out.push(i);
    };
    for (const op of spec.ops) opImages(op).forEach(add);
    spec.afterImages?.forEach(add);
    return out;
  }

  /** Loads the images, then paints. */
  async bake(spec: BakeSpec): Promise<BakedSkin> {
    await this.images.preload(this.imagesFor(spec));
    return this.bakeNow(spec);
  }

  /** Paints with the images that are cached right now. */
  bakeNow(spec: BakeSpec): BakedSkin {
    const missing = new Set<string>();
    const get = (path: string, mode: KeyMode): PixelImage | null => {
      const img = this.images.peek(path, mode);
      if (img === undefined || img === MISSING) {
        missing.add(normalizePath(path));
        return null;
      }
      return img;
    };

    // A single-image frame background is painted at its natural size, which may exceed the window size.
    let width = spec.width;
    let height = spec.height;
    for (const op of spec.ops) {
      if (op.op === 'image' && op.x === 0 && op.y === 0) {
        const bg = this.images.peek(themePath(op.texture), 'nibble');
        if (bg !== undefined && bg !== MISSING) {
          width = Math.max(width, bg.width);
          height = Math.max(height, bg.height);
        }
      }
    }
    const buf = createBuffer(width, height);

    const tile9 = (texture: string, rect: Rect): void => {
      const tiles = tileSetNames(texture, 9).map((t) => get(themePath(t), 'nibble'));
      for (const cell of tile9Cells(rectWidth(rect), rectHeight(rect))) {
        const img = tiles[cell.tile];
        if (img) blit(buf, img, rect.l + cell.x, rect.t + cell.y);
      }
    };

    for (const op of spec.ops) {
      switch (op.op) {
        case 'frame': {
          const tiles = tileSetNames(op.texture, 12).map((t) => get(themePath(t), 'nibble'));
          const cols = Math.floor(op.width / FRAME_TILE);
          const rows = Math.floor(op.height / FRAME_TILE);
          for (let y = 0; y < rows; y += 1) {
            for (let x = 0; x < cols; x += 1) {
              const img = tiles[frameTileIndex(x, y, cols, rows)];
              if (img) blit(buf, img, x * FRAME_TILE, y * FRAME_TILE);
            }
          }
          break;
        }
        case 'image': {
          const img = get(themePath(op.texture), 'nibble');
          if (img) blit(buf, img, op.x, op.y);
          break;
        }
        case 'tile9':
          tile9(op.texture, op.rect);
          break;
        case 'tabs': {
          const panel: Rect = { ...op.rect, r: op.rect.r - TAB_RIGHT_MARGIN };
          tile9(DEF_CTRL_TEXT, panel);
          // the strip: the bottom 2 x 8 px rows of the panel
          const rowsTotal = Math.floor(rectHeight(op.rect) / CONTROL_TILE_STEP);
          const stripTop = op.rect.t + (rowsTotal - 2) * CONTROL_TILE_STEP;
          const cols = Math.floor(rectWidth(panel) / CONTROL_TILE_STEP);
          const tabTiles = tileSetNames(TAB_TILE_SET, 6).map((t) => get(themePath(t), 'nibble'));
          for (const s of tabStripTiles(cols, op.tabs)) {
            const img = tabTiles[s.tile];
            if (img) blit(buf, img, op.rect.l + s.col * CONTROL_TILE_STEP, stripTop);
          }
          break;
        }
        case 'untextured': {
          const paper = get(themePath('WindField.bmp'), 'exact');
          const w = op.width;
          const h = op.height;
          if (paper) {
            tileFill(buf, paper, { l: 0, t: 0, r: w, b: Math.min(h, 20) });
            tileFill(buf, paper, { l: 0, t: Math.max(0, h - 4), r: w, b: h });
            tileFill(buf, paper, { l: 0, t: 20, r: 4, b: Math.max(20, h - 4) });
            tileFill(buf, paper, { l: w - 4, t: 20, r: w, b: Math.max(20, h - 4) });
          }
          outline(buf, { l: 0, t: 0, r: w, b: h }, [0, 0, 0], true);
          outline(buf, { l: 1, t: 1, r: w - 1, b: h - 1 }, [160, 160, 160], true);
          outline(buf, { l: 2, t: 2, r: w - 2, b: h - 2 }, [255, 255, 255], false);
          outline(buf, { l: 3, t: 3, r: w - 3, b: h - 3 }, [160, 160, 160], false);
          if (op.caption) {
            // RenderWndBaseTitleBar: 21 px, white -> 150 over the first 40 %, 150 -> 230 over the rest
            const first = Math.floor((21 * 40) / 100);
            verticalGradient(buf, 0, 0, w, first, [255, 255, 255], [150, 150, 150]);
            verticalGradient(buf, 0, first, w, 21, [150, 150, 150], [230, 230, 230]);
          }
          break;
        }
      }
    }

    spec.after?.({
      buffer: buf,
      image: (path, mode = 'nibble') => this.images.peek(path, mode),
      blit: (path, x, y, mode = 'nibble') => {
        const img = get(path, mode);
        if (img) blit(buf, img, x, y);
      },
      tile9,
    });
    return { width: buf.width, height: buf.height, data: buf.data, missing: [...missing] };
  }
}
