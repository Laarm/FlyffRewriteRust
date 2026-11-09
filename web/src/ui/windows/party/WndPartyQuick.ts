// `WndPartyQuick` (`CWndPartyQuick`, `src:_Interface/WndPartyQuick.cpp`, ref/22 §17 C, Plan/42 §19.4 C, 45 §7.8; WP-UI-39): `APP_PARTY_QUICK`
// (144 x 336), the simple party display. One 112 x 24 box per member (`WIDC_STATIC_MEM1..8`): "<level>. <name>" in a colour that says how the
// member is (black fine, yellow under 10 % HP, red dead, grey out of view, black offline, green for the leader) and a thin HP gauge under
// it. A click on a box makes the member the target (the own box clears it). The window grows with the party (60 + 34 px per member); the
// small button (`WIDC_BUTTON1`) opens or closes the detailed party window. It is only created with two or more members.

import { APP_PARTY_QUICK, WIDC_BUTTON1 } from '../../../gen/defs/resdata';
import { Gauge } from '../../controls/Gauge';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { LEADER_COLOR, quickButtonRect, quickHeight, quickText } from './partyGeometry';
import { EMPTY_PARTY_VIEW, type PartyActions, type PartyView, type ViewSource } from './ports';
import { RectLayer, type RectEntry } from './RectLayer';

export interface PartyQuickDeps {
  readonly view: ViewSource<PartyView>;
  readonly actions: PartyActions;
}

/** The theme files of the thin gauge. */
export const QUICK_GAUGE_EMPTY = 'GauEmptySmall.bmp';
export const QUICK_GAUGE_FILL = 'GauFillSmall.bmp';
/** `0x60ffff00`: the highlight of the member that is the target. */
const FOCUS_FILL = 0x60ffff00;
const COLOR = {
  ok: 0xff000000,
  low: 0xffffff00,
  dead: 0xffff0000,
  away: 0xff878787,
  offline: 0xff000000,
} as const;
const ID_RECTS = 0x7d31;
const ID_TEXTS = 0x7d32;
const ID_GAUGE0 = 0x7d40;
const MAX_MEMBERS = 8;

export class WndPartyQuick extends WndNeuz {
  readonly appId = APP_PARTY_QUICK;
  readonly #deps: PartyQuickDeps;
  #rects: RectLayer | null = null;
  #texts: TextLayer | null = null;
  readonly #gauges: Gauge[] = [];
  #applied: PartyView = EMPTY_PARTY_VIEW;
  /** `m_pFocusMember`: the member that was clicked last (a character id), `0` for none. */
  #focus = 0;
  /** `m_MemberCount`: the number of boxes shown. */
  #count = -1;
  #unsubscribe: (() => void) | null = null;

  constructor(deps: PartyQuickDeps) {
    super();
    this.#deps = deps;
  }

  /** `if( g_Party.GetSizeofMember() < 2 ) return FALSE;`: without two members there is no window. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    if (this.#deps.view.get().members.length < 2) return false;
    return super.initialize(parent, extraStyle);
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
  get partyButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  memberBox(i: number): Static | null {
    return this.getDlgItem<Static>(`WIDC_STATIC_MEM${String(i + 1)}`);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.getClientRect(false);
    this.#rects = this.addControl(new RectLayer(), ID_RECTS, client);
    for (let i = 0; i < MAX_MEMBERS; i += 1) {
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
    this.#apply(this.#deps.view.get());
    this.#unsubscribe = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    // `Move( CPoint( 30, ( rectRoot.bottom - rectRoot.top ) / 4 ) )`
    const layout = this.env.wm.layoutRect;
    this.move({ x: 30, y: Math.trunc((layout.b - layout.t) / 4) });
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  /** The layout rect of the box of member `i` (`GetWndCtrl( StaticID[i] )->rect`). */
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

  /** `SetActiveMember`: the boxes of members `0 .. n - 1` are shown, the window is `60 + 34 n` high, the button moves under them. */
  #setActiveMember(n: number): void {
    if (n === this.#count) return;
    this.#count = n;
    for (let i = 0; i < MAX_MEMBERS; i += 1) {
      const box = this.memberBox(i);
      box?.enableWindow(i < n);
      box?.setVisible(i < n);
    }
    const height = quickHeight(n);
    const r = this.getWindowRect(true);
    this.setWndRect({ l: r.l, t: r.t, r: r.r, b: r.t + height }, true);
    this.partyButton?.setWndRect(quickButtonRect(height));
    this.requestBake();
  }

  #apply(v: PartyView): void {
    this.#applied = v;
    const n = v.members.length;
    this.#setActiveMember(n);
    const rects: RectEntry[] = [];
    const items: TextItem[] = [];
    v.members.forEach((m, i) => {
      const box = this.#boxRect(i);
      if (m.id === this.#focus) {
        rects.push({
          x: box.l,
          y: box.t,
          w: box.r - box.l,
          h: box.b - box.t,
          fill: FOCUS_FILL,
        });
      }
      let color: number = COLOR[m.status];
      if (m.inView && m.leader) color = LEADER_COLOR;
      items.push({ x: box.l + 5, y: box.t + 5, text: quickText(m.quickText), color });
    });
    this.#rects?.setRects(rects);
    this.#texts?.setItems(items);
    for (let i = 0; i < this.#gauges.length; i += 1) {
      const g = this.#gauges[i];
      const m = v.members[i];
      if (!g) continue;
      g.setVisible(m !== undefined);
      g.set(m ? Math.round(m.hp * QUICK_STEPS) : 0, QUICK_STEPS);
    }
  }

  /** `OnLButtonUp`: the box under the pointer makes its member the target; the own box clears the target. */
  override onLButtonUp(pt: Point): void {
    const v = this.#applied;
    for (let i = 0; i < v.members.length; i += 1) {
      if (!rectContains(this.#boxRect(i), pt)) continue;
      const m = v.members[i];
      if (!m) return;
      this.#focus = m.id;
      this.#apply(v);
      this.#deps.actions.focusMember(m.id);
      return;
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.#deps.actions.toggleParty();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    super.onDestroy();
  }
}

const QUICK_STEPS = 1000;
