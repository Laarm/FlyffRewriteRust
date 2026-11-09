// `PartyInfoPage` (`CWndPartyInfo`, `src:_Interface/WndParty.cpp:241-600`, ref/22 §17 H, Plan/42 §19.4 H, 45 §22.2; WP-UI-39): the "Information"
// tab of `APP_PARTY` (`APP_PARTY_INFO`, a borderless page of the tab control). One line per member in slot order: a status square (blue fine,
// yellow under 10 % HP, red dead, grey out of view, black offline), the level (`75`, `75-M`, `75-H`), "L" / "M" (leader / member), the job, the
// name and the HP gauge of a member in view; the leader's line is green. A press selects the row (it highlights, the member becomes the
// target when it is in view; a press beside the rows clears the target); the window's Leave / hand-over buttons act on the selected row.

import { Gauge } from '../../controls/Gauge';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Point } from '../../layout/geometry';
import { CodePage } from '../char/CodePage';
import {
  infoGaugeRect,
  infoRowAt,
  infoSelectRect,
  infoSquare,
  INFO_COL,
  INFO_ROW_PITCH,
  INFO_SELECT_FILL,
  infoRowY,
  LEADER_COLOR,
  STATUS_COLOR,
  TEXT_COLOR,
} from './partyGeometry';
import type { PartyActions, PartyView, ViewSource } from './ports';
import { RectLayer, type RectEntry } from './RectLayer';

/** The theme file of the gauge (both layers are loaded from the "Empty" bitmap, the fill is red by its vertex colour). */
export const PARTY_GAUGE_TEXTURE = 'GauEmptyNormal.bmp';
/** `RenderGauge( ..., 0x64ff0000, ...)`. */
export const PARTY_GAUGE_COLOR = 0x64ff0000;

const ID_RECTS = 0x7d01;
const ID_TEXTS = 0x7d02;
const ID_GAUGE0 = 0x7d10;

export interface PartyInfoDeps {
  readonly view: ViewSource<PartyView>;
  readonly actions: PartyActions;
}

export class PartyInfoPage extends CodePage {
  readonly #deps: PartyInfoDeps;
  #rects: RectLayer | null = null;
  #texts: TextLayer | null = null;
  readonly #gauges: Gauge[] = [];
  #applied: PartyView | null = null;
  /** `m_nSelected`: the selected row, `-1` for none. */
  #selected = -1;

  constructor(deps: PartyInfoDeps) {
    super();
    this.#deps = deps;
  }

  /** The selected row (`m_nSelected`), `-1` for none. */
  get selected(): number {
    return this.#selected;
  }
  /** The character id of the selected row, `null` for none. */
  get selectedId(): number | null {
    return this.#applied?.members[this.#selected]?.id ?? null;
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

  /** Clears the selection (the Leave button does after it acted). */
  clearSelection(): void {
    if (this.#selected === -1) return;
    this.#selected = -1;
    if (this.#applied) this.#apply(this.#applied);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.getClientRect(false);
    this.#rects = this.addControl(new RectLayer(), ID_RECTS, client);
    for (let i = 0; i < 8; i += 1) {
      const g = this.addControl(
        new Gauge({
          background: PARTY_GAUGE_TEXTURE,
          fill: PARTY_GAUGE_TEXTURE,
          color: PARTY_GAUGE_COLOR,
        }),
        ID_GAUGE0 + i,
        infoGaugeRect(i),
      );
      g.setVisible(false);
      this.#gauges.push(g);
    }
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.#apply(this.#deps.view.get());
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  /** The items of the rows: `OnDraw` for the view. */
  #apply(v: PartyView): void {
    this.#applied = v;
    // `if( g_Party.GetSizeofMember() < m_nSelected + 1 ) m_nSelected = -1;`
    if (this.#selected >= v.members.length) this.#selected = -1;
    const rects: RectEntry[] = [];
    const items: TextItem[] = [];
    if (this.#selected >= 0) {
      const r = infoSelectRect(this.#selected);
      rects.push({ x: r.l, y: r.t, w: r.r - r.l, h: r.b - r.t, fill: INFO_SELECT_FILL });
    }
    v.members.forEach((m, i) => {
      const sq = infoSquare(i);
      rects.push({
        x: sq.outer.l,
        y: sq.outer.t,
        w: sq.outer.r - sq.outer.l,
        h: sq.outer.b - sq.outer.t,
        fill: TEXT_COLOR,
      });
      rects.push({
        x: sq.inner.l,
        y: sq.inner.t,
        w: sq.inner.r - sq.inner.l,
        h: sq.inner.b - sq.inner.t,
        fill: STATUS_COLOR[m.status],
      });
      const y = infoRowY(i);
      const color = m.leader ? LEADER_COLOR : TEXT_COLOR;
      items.push({ x: INFO_COL.level, y, text: m.levelText, color });
      items.push({ x: INFO_COL.cls, y, text: m.classText, color });
      items.push({ x: INFO_COL.job, y, text: m.jobName, color });
      items.push({ x: INFO_COL.name, y, text: m.name, color });
    });
    this.#rects?.setRects(rects);
    this.#texts?.setItems(items);
    for (let i = 0; i < this.#gauges.length; i += 1) {
      const g = this.#gauges[i];
      const m = v.members[i];
      if (!g) continue;
      g.setVisible(m !== undefined);
      // the empty bar is always drawn; the fill follows the HP of a member in view
      g.set(m ? Math.round(m.hp * INFO_GAUGE_STEPS) : 0, INFO_GAUGE_STEPS);
    }
  }

  /** `OnLButtonDown`: select the row under the pointer; a member in view becomes the target, anything else clears it. */
  override onLButtonDown(pt: Point): void {
    const v = this.#applied ?? this.#deps.view.get();
    const row = infoRowAt(pt.x, pt.y, v.members.length);
    this.#selected = row;
    this.#apply(v);
    this.#deps.actions.focusMember(row >= 0 ? (v.members[row]?.id ?? null) : null);
  }
}

/** The gauge value has this many steps (`width * value / max` in whole pixels of the 110 px bar). */
const INFO_GAUGE_STEPS = 1000;

export { INFO_ROW_PITCH };
