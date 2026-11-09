// `WndCommItem` (`CWndCommItem` + `CWndCommItemCtrl`, `src:_Interface/WndCommItem.cpp:1-1100`, ref/22 §17 D, Plan/42 §19.4 D, 45 §12;
// WP-UI-28): `APP_COMM_ITEM` (256 x 256), the "item duration" list. One tab, `TID_TOOLTIP_ITEMTIME`, whose page lists the premium items
// in effect: a 32 x 32 picture at x = 2, the name in blue at (40, y + 3), the remaining time under it at (40, y + 18), 30 px per row,
// the rows start at y = 3, the scroll bar pages by `clientHeight / 30`. The rows (state-mode items and charged item buffs, the
// merged experience scrolls, the ticket, the feed pocket) are the buff feature's; the window draws them and puts their tooltip.

import { APP_COMM_ITEM, WIDC_TABCTRL1 } from '../../../gen/defs/resdata';
import { TID_TOOLTIP_ITEMTIME } from '../../../gen/defs/text';
import { IconStrip, type StripEntry } from '../../controls/IconStrip';
import { createOwnerScrollBar, type ScrollBar } from '../../controls/ScrollBar';
import type { TabCtrl } from '../../controls/TabCtrl';
import { type Point, rectHeight, rectOffset } from '../../layout/geometry';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CodePage } from '../char/CodePage';
import type { CommItemRequest, CommItemRow } from './ports';

/** The row pitch of `m_nFontHeight`. */
export const COMM_ROW_HEIGHT = 30;
const NAME_COLOR = 0xff0000ff;
const DETAIL_COLOR = 0xff000000;

export interface CommItemDeps {
  readonly request: CommItemRequest;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

/** The entries of the visible rows of a list (a pure function, for the tests). */
export function commItemEntries(
  rows: readonly CommItemRow[],
  first: number,
  count: number,
): StripEntry[] {
  const out: StripEntry[] = [];
  for (let i = first; i < Math.min(rows.length, first + count); i += 1) {
    const row = rows[i];
    if (!row) continue;
    const y = 3 + (i - first) * COMM_ROW_HEIGHT;
    out.push({
      key: row.key,
      icon: row.icon,
      x: 2,
      y,
      w: 32,
      h: 32,
      alpha: 192,
      texts: [
        { x: 40, y: y + 3, text: row.name, color: NAME_COLOR },
        { x: 40, y: y + 18, text: row.detail, color: DETAIL_COLOR },
      ],
    });
  }
  return out;
}

/** `CWndCommItemCtrl`: the list page of the tab. */
export class CommItemPage extends CodePage {
  readonly #deps: CommItemDeps;
  #strip: IconStrip | null = null;
  #bar: ScrollBar | null = null;
  #rows: readonly CommItemRow[] = [];
  #applied: unknown = null;
  #hover = -1;

  constructor(deps: CommItemDeps) {
    super();
    this.#deps = deps;
  }

  get strip(): IconStrip | null {
    return this.#strip;
  }
  get scrollBar(): ScrollBar | null {
    return this.#bar;
  }
  get rows(): readonly CommItemRow[] {
    return this.#rows;
  }

  get pageSize(): number {
    return Math.max(1, Math.floor(rectHeight(this.getClientRect(false)) / COMM_ROW_HEIGHT));
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#strip = this.addControl(new IconStrip(), 0x7d01, this.getClientRect(false));
    const bar = createOwnerScrollBar(this);
    bar.onScroll = () => {
      this.#paint();
    };
    bar.forwardWheel = (n) => {
      this.onMouseWheel(n);
    };
    this.#bar = bar;
    this.#apply();
  }

  override process(now: number): void {
    const v = this.#deps.request.view.get();
    if (v !== this.#applied) this.#apply();
    super.process(now);
  }

  #apply(): void {
    const v = this.#deps.request.view.get();
    this.#applied = v;
    this.#rows = v.rows;
    const page = this.pageSize;
    const bar = this.#bar;
    if (bar) {
      bar.setScrollRange(0, Math.max(v.rows.length, page));
      bar.setScrollPage(page);
      if (bar.getScrollPos() > Math.max(0, v.rows.length - page))
        bar.setScrollPos(Math.max(0, v.rows.length - page));
    }
    this.#paint();
  }

  #paint(): void {
    const first = this.#bar?.getScrollPos() ?? 0;
    this.#strip?.setEntries(commItemEntries(this.#rows, first, this.pageSize + 1));
  }

  override onMouseWheel(notches: number): void {
    const bar = this.#bar;
    if (!bar) return;
    const max = Math.max(0, this.#rows.length - this.pageSize);
    bar.setScrollPos(Math.max(0, Math.min(max, bar.getScrollPos() - notches)));
  }

  /** `OnMouseWndSurface`: the tooltip of the row under the pointer (over its picture). */
  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const first = this.#bar?.getScrollPos() ?? 0;
    const rel = pt.y - 3;
    const idx = rel < 0 ? -1 : Math.floor(rel / COMM_ROW_HEIGHT);
    const row = idx >= 0 && pt.x >= 2 && pt.x < 34 ? this.#rows[first + idx] : undefined;
    if (!row) {
      if (this.#hover >= 0) {
        this.#hover = -1;
        this.#deps.tooltip.cancel();
      }
      return;
    }
    this.#hover = first + idx;
    const s = this.getScreenClientRect();
    const rect = rectOffset(
      { l: 2, t: 3 + idx * COMM_ROW_HEIGHT, r: 34, b: 35 + idx * COMM_ROW_HEIGHT },
      s.l,
      s.t,
    );
    this.#deps.tooltip.put(this, row.tooltip, rect, 0);
  }
}

export class WndCommItem extends WndNeuz {
  readonly appId = APP_COMM_ITEM;
  readonly #deps: CommItemDeps;
  #page: CommItemPage | null = null;

  constructor(deps: CommItemDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get page(): CommItemPage | null {
    return this.#page;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      const page = new CommItemPage(this.#deps);
      page.attach(tab, 398);
      this.#page = page;
      tab.insertItem(0, { text: this.#deps.getText(TID_TOOLTIP_ITEMTIME), page });
      tab.setCurSel(0);
    }
    this.moveParentCenter();
  }
}
