// `WndSecretRoomQuick` (`CWndSecretRoomQuick`, `src:_Interface/WndSecretRoom.cpp:1838-2264`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-57): `APP_SECRETROOM_QUICK` (754, 144 x 416), the member
// list of the own line-up during the war. One 112 x 24 box per member (`WIDC_STATIC_MEM1 .. 11`, at most `MAX_SECRETROOM_MEMBER`): the text (cut over 13 characters), its colour and a thin
// HP gauge 18 px under it come from the request's view (the feature builds them from the guild roster and the movers in view). The window is `40 + 34 n` high for `n` rows and opens at
// the right edge of the layout, y 160. A click on a row asks the feature (`QuickActions.click`): another member in view becomes the target, the own row clears the target, a member out of view
// changes nothing, and nothing happens while the player auto-attacks; a taken click makes the member the clicked one, whose row gets the `0x60ffff00` band while he is in view.
//
// The title-bar CLOSE button (`WTBID_CLOSE`) does NOT close (KEEP): it toggles the mini mode, an 80 px window with only the clicked member (when he is in view, else the first row); with one row
// nothing happens. Esc closes the window (the feature opens it again at the next check).

import { APP_SECRETROOM_QUICK } from '../../../gen/defs/resdata';
import { Gauge } from '../../controls/Gauge';
import { argbCss, RectLayer, type RectEntry } from '../../controls/RectLayer';
import type { Static } from '../../controls/Static';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains } from '../../layout/geometry';
import { Wnm, WTBID_CLOSE } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { EMPTY_QUICK_VIEW, type QuickRequest, type QuickRowView, type QuickView } from './ports';
import {
  cutOver,
  MAX_SECRETROOM_MEMBER,
  QUICK_FOCUS_FILL,
  QUICK_MINI_HEIGHT,
  QUICK_TEXT_CUT,
  QUICK_Y,
  quickHeight,
} from './rows';

/** The theme files of the thin gauge. */
export const QUICK_GAUGE_EMPTY = 'GauEmptySmall.bmp';
export const QUICK_GAUGE_FILL = 'GauFillSmall.bmp';
const ID_RECTS = 0x7d51;
const ID_TEXTS = 0x7d52;
const ID_GAUGE0 = 0x7d60;
const STEPS = 1000;

export class WndSecretRoomQuick extends WndNeuz {
  readonly appId = APP_SECRETROOM_QUICK;
  readonly #request: QuickRequest;
  #rects: RectLayer | null = null;
  #texts: TextLayer | null = null;
  readonly #gauges: Gauge[] = [];
  #applied: QuickView = EMPTY_QUICK_VIEW;
  /** The rows now drawn (the mini mode shows one of them). */
  #shown: readonly QuickRowView[] = [];
  /** `m_FocusMemberid`: the member that was clicked last (a character id), 0 for none. */
  #focus = 0;
  #mini = false;
  /** `m_MemberCount`: the number of boxes shown. */
  #count = -1;
  #unsubscribe: (() => void) | null = null;

  constructor(request: QuickRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get rectLayer(): RectLayer | null {
    return this.#rects;
  }
  get textLayer(): TextLayer | null {
    return this.#texts;
  }
  get gauges(): readonly Gauge[] {
    return this.#gauges;
  }
  /** The number of member boxes that are shown. */
  get memberCount(): number {
    return this.#count;
  }
  get focusId(): number {
    return this.#focus;
  }
  get isMini(): boolean {
    return this.#mini;
  }
  /** The rows now drawn. */
  get shownRows(): readonly QuickRowView[] {
    return this.#shown;
  }
  memberBox(i: number): Static | null {
    return this.getDlgItem<Static>(`WIDC_STATIC_MEM${String(i + 1)}`);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.getClientRect(false);
    this.#rects = this.addControl(new RectLayer(), ID_RECTS, client);
    for (let i = 0; i < MAX_SECRETROOM_MEMBER; i += 1) {
      const box = this.#boxRect(i);
      const g = this.addControl(
        new Gauge({ background: QUICK_GAUGE_EMPTY, fill: QUICK_GAUGE_FILL, color: 0x64ff0000 }),
        ID_GAUGE0 + i,
        // `rect.TopLeft().y += 18; x += 2; right -= 2`
        { l: box.l + 2, t: box.t + 18, r: box.r - 2, b: box.b },
      );
      g.setVisible(false);
      this.#gauges.push(g);
    }
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.#apply(this.#request.view.get());
    this.#unsubscribe = this.#request.view.subscribe(() => {
      this.#apply(this.#request.view.get());
    });
    // the right edge of the layout, y 160
    const layout = this.env.wm.layoutRect;
    const w = this.getWindowRect(true);
    this.move({ x: layout.r - (w.r - w.l), y: QUICK_Y });
  }

  override process(now: number): void {
    const v = this.#request.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  /** The layout rect of the box of slot `i` (`GetWndCtrl( StaticID[i] )->rect`). */
  #boxRect(i: number): Rect {
    return (
      this.getWndCtrl(`WIDC_STATIC_MEM${String(i + 1)}`)?.rect ?? {
        l: 10,
        t: 8 + 34 * i,
        r: 122,
        b: 32 + 34 * i,
      }
    );
  }

  /** The rows the mode draws: all of them, or in the mini mode the clicked member (in view) or the first row. */
  #visibleRows(v: QuickView): readonly QuickRowView[] {
    if (!this.#mini) return v.rows.slice(0, MAX_SECRETROOM_MEMBER);
    const clicked = v.rows.find((r) => r.id === this.#focus && r.inView);
    const row = clicked ?? v.rows[0];
    return row ? [row] : [];
  }

  /** `SetActiveMember`: the boxes of slots `0 .. n - 1` are shown and the window is `40 + 34 n` high (80 in the mini mode). */
  #setActiveMember(n: number): void {
    if (n === this.#count) return;
    this.#count = n;
    for (let i = 0; i < MAX_SECRETROOM_MEMBER; i += 1) {
      const box = this.memberBox(i);
      box?.enableWindow(i < n);
      box?.setVisible(i < n);
    }
    const height = this.#mini ? QUICK_MINI_HEIGHT : quickHeight(n);
    const r = this.getWindowRect(true);
    this.setWndRect({ l: r.l, t: r.t, r: r.r, b: r.t + height }, true);
    this.requestBake();
  }

  #apply(v: QuickView): void {
    this.#applied = v;
    const rows = this.#visibleRows(v);
    this.#shown = rows;
    this.#setActiveMember(rows.length);
    const rects: RectEntry[] = [];
    const items: TextItem[] = [];
    rows.forEach((m, i) => {
      const box = this.#boxRect(i);
      if (m.id === this.#focus && m.inView) {
        rects.push({
          key: `focus${String(i)}`,
          x: box.l,
          y: box.t,
          w: box.r - box.l,
          h: box.b - box.t,
          fill: argbCss(QUICK_FOCUS_FILL),
        });
      }
      items.push({
        x: box.l + 5,
        y: box.t + 5,
        text: cutOver(m.text, QUICK_TEXT_CUT),
        color: m.color,
      });
    });
    this.#rects?.setEntries(rects);
    this.#texts?.setItems(items);
    for (let i = 0; i < this.#gauges.length; i += 1) {
      const g = this.#gauges[i];
      const m = rows[i];
      if (!g) continue;
      g.setVisible(m !== undefined);
      g.set(m ? Math.round(Math.max(0, Math.min(1, m.hp)) * STEPS) : 0, STEPS);
    }
  }

  /** The title CLOSE button: toggles the mini mode (with one row it does not go into it; it always leaves it). */
  toggleMini(): void {
    if (!this.#mini && this.#applied.rows.length <= 1) return;
    this.#mini = !this.#mini;
    this.#count = -1;
    this.#apply(this.#applied);
  }

  /** `OnLButtonUp`: the row under the pointer. */
  override onLButtonUp(pt: Point): void {
    for (let i = 0; i < this.#shown.length; i += 1) {
      if (!rectContains(this.#boxRect(i), pt)) continue;
      const m = this.#shown[i];
      if (!m) return;
      if (this.#request.actions.click(m.id)) {
        this.#focus = m.id;
        this.#apply(this.#applied);
      }
      return;
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WTBID_CLOSE) {
      this.toggleMini();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
