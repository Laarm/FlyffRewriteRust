// `CursorManager` (Plan/42 §15; `CNeuzApp::SetDeviceCursor`, `CWndBase::OnSetCursor`): maps cursor ids to
// `/gen/cursor/<fp>/<stem>.png` + hotspot and applies the CSS `cursor: url(...) hx hy, auto` to the UI root (windows)
// or the 3-D canvas (the world, chosen by 43 through `setWorldCursor`). A cursor without a file falls back to the CSS
// default. The `url(...)` may be the RAMPED data URL of `UiRamp` (WP-UI-09, Plan/42 §7.8 rule 7): `setUrlTransform`.
//
// UI rules (`attach`): windows show `CUR_BASE`; an editable edit shows `CUR_EDIT`; the border of a `WBS_THICKFRAME`
// frame shows the resize cursors; nothing changes while a button is held; a dragged shortcut leaves `CUR_BASE`.

import { EditCtrl } from '../controls/EditCtrl';
import { type Point, rectHeight, rectWidth } from '../layout/geometry';
import { EBS_READONLY, WBS_CHILD, WBS_THICKFRAME } from '../layout/styles';
import { resizeDir } from '../wm/dragMath';
import type { WindowManager } from '../wm/WindowManager';
import type { WndBase } from '../wnd/WndBase';
import { CUR, CURSORS, type CursorId, cursorStem, resizeCursor } from './cursors';

/** The part of the asset host the manager needs (`UiHost.assets`). */
export interface CursorAssets {
  cursorUrl?(stem: string): string | null;
  cursorHotspot?(stem: string): [number, number] | null;
}

/** Re-maps a cursor URL (the ramped data URL); `null` = keep the original. */
export type CursorUrlTransform = (stem: string, url: string) => string | null;

/** The cursor a window asks for through its optional `uiCursor()` hook (the window or its frame), `null` for none. */
function ownCursor(target: WndBase): number | null {
  for (let w: WndBase | null = target; w; w = w.parent) {
    const fn = (w as { uiCursor?: () => number | null }).uiCursor;
    if (typeof fn === 'function') return fn.call(w);
    if (!w.isWndStyle(WBS_CHILD)) break;
  }
  return null;
}

/** What a missing cursor file falls back to: the browser's default cursor. */
export const FALLBACK_CURSOR = 'auto';

export class CursorManager {
  readonly #assets: CursorAssets;
  #transform: CursorUrlTransform | null = null;
  #uiTarget: HTMLElement | null = null;
  #worldTarget: HTMLElement | null = null;
  #ui: number = CUR.BASE;
  #world: number = CUR.BASE;

  constructor(assets: CursorAssets = {}) {
    this.#assets = assets;
  }

  /** The CSS `cursor` value of cursor `id` (`url("...") hx hy, auto`, or `auto` when the file is missing). */
  css(id: number): string {
    const stem = cursorStem(id);
    if (stem === null) return FALLBACK_CURSOR;
    const raw = this.#assets.cursorUrl?.(stem) ?? null;
    if (raw === null) return FALLBACK_CURSOR;
    const url = this.#transform?.(stem, raw) ?? raw;
    const hs = this.#assets.cursorHotspot?.(stem);
    const spot = hs ? ` ${String(hs[0])} ${String(hs[1])}` : '';
    return `url("${url.replace(/"/g, '%22')}")${spot}, ${FALLBACK_CURSOR}`;
  }

  /** `UiRamp` replaces the cursor images by ramped copies: a new transform re-applies the current cursors. */
  setUrlTransform(fn: CursorUrlTransform | null): void {
    this.#transform = fn;
    this.refresh();
  }

  /** The element that carries the UI cursor (the UI root; windows inherit it). */
  setUiTarget(el: HTMLElement | null): void {
    this.#uiTarget = el;
    this.refresh();
  }

  /** The element that carries the world cursor (the 3-D canvas, 43). */
  setWorldTarget(el: HTMLElement | null): void {
    this.#worldTarget = el;
    this.refresh();
  }

  get uiCursor(): number {
    return this.#ui;
  }
  get worldCursor(): number {
    return this.#world;
  }

  /** The window cursor (`SetMouseCursor` of a window): applied to the UI root. */
  setUiCursor(id: number): void {
    if (this.#ui === id) return;
    this.#ui = id;
    this.#apply(this.#uiTarget, id);
  }

  /** `setWorldCursor(id)`, called by 43 with its `OnSetCursor` table. */
  setWorldCursor(id: number): void {
    if (this.#world === id) return;
    this.#world = id;
    this.#apply(this.#worldTarget, id);
  }

  /** Applies both cursors again (after a target or the ramp changed). */
  refresh(): void {
    this.#apply(this.#uiTarget, this.#ui);
    this.#apply(this.#worldTarget, this.#world);
  }

  #apply(el: HTMLElement | null, id: number): void {
    if (el) el.style.cursor = this.css(id);
  }

  /** The cursor a window under the pointer asks for (`OnSetCursor` of the window classes). */
  uiCursorFor(target: WndBase | null, pt: Point): CursorId {
    if (!target) return CUR.BASE;
    // a window that paints its own hot spots (the NPC dialog's words) names its cursor (`SetMouseCursor` of `OnMouseWndSurface`)
    const own = ownCursor(target);
    if (own !== null) return own as CursorId;
    if (!target.isWndStyle(WBS_CHILD) && target.isWndStyle(WBS_THICKFRAME)) {
      const r = target.getScreenRect();
      const dir = resizeDir({ x: pt.x - r.l, y: pt.y - r.t }, rectWidth(r), rectHeight(r));
      const c = resizeCursor(dir);
      if (c !== null) return c;
    }
    if (target instanceof EditCtrl && !target.isWndStyle(EBS_READONLY)) return CUR.EDIT;
    return CUR.BASE;
  }

  /** Follows the pointer: sets the UI cursor from the window under it (not while a button is held). */
  attach(wm: WindowManager): () => void {
    return wm.onEvent((e) => {
      if (e.type === 'hover' && e.target === null) {
        this.setUiCursor(CUR.BASE);
      } else if (e.type === 'pointerMove' && !wm.pointerPressed) {
        this.setUiCursor(wm.isDraggingShortcut() ? CUR.BASE : this.uiCursorFor(wm.hover, e.pt));
      }
    });
  }

  /** The ids with their stems (for the gallery and the debug probe). */
  table(): readonly { id: number; name: string; stem: string; css: string }[] {
    return CURSORS.map((c) => ({ id: c.id, name: c.name, stem: c.stem, css: this.css(c.id) }));
  }
}
