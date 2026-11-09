// `WndHeavenTower` (`CWndHeavenTower`, `src:_Interface/WndField.cpp:22170-22445`, ref/22 §17 G, Plan/42 §19.4 G, 45 §17; WP-UI-53): `APP_HEAVEN_TOWER` (734, 304 x 416), the floor list of the NPC menu
// `MMI_HEAVEN_TOWER`. Seven rows `"%d. %s"` (`TID_GAME_HEAVETOWER_FLOOR01..07`), the description `TID_TOOLTIP_HEAVENTOWER` on one page (so `WIDC_BUTTON1`, the page-down button, is hidden and disabled).
//
//   * `gap = height( "123" ) + 2`, `offset = gap >= 16 ? gap - 16 : 0`; the list gets the height `7 * gap + 10`, the window `listHeight + textHeight + 60`; the list, the text and the window are 80 px
//     wider (the English texts) and the button moves 80 px; the selection band ARGB( 60, 240, 0, 0 ) lies over `WIDC_CUSTOM2` shifted by `choice * gap + offset` (80 px wider) and stays drawn while
//     the confirmation is open;
//   * a click on a row selects it: row 6 ("Do not Enter.") closes the window; rows 0-4 open the entrance confirmation (OK / Cancel) with `FLOORnn_MSG` formatted with the cost, row 5 with
//     `FLOOR06_MSG` (no cost);
//   * OK: floor 1..5 -> `request.enter( floor )` and the tower closes; floor 6 sends nothing (KEEP `m_nFloor != 6`); the box closes; Cancel closes the box only.
//
// `chosen` is the selected row (-1 before a pick) for ui-secret-tax's probe `features.heavenTower`. The window is centred.

import {
  APP_HEAVEN_TOWER,
  WIDC_BUTTON1,
  WIDC_CUSTOM2,
  WIDC_LISTBOX1,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { TID_TOOLTIP_HEAVENTOWER } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { ListBox } from '../../controls/ListBox';
import { RectLayer } from '../../controls/RectLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Rect, rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME, Wnm } from '../../layout/styles';
import { FONT, lineHeight } from '../../text/fonts';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { HeavenTowerRequest, QuestHost } from './ports';
import {
  HEAVEN_TOWER_CLOSE_ROW,
  HEAVEN_TOWER_COSTS,
  HEAVEN_TOWER_FREE_ROW,
  HEAVEN_TOWER_MSG_TIDS,
  HEAVEN_TOWER_WIDEN,
  heavenTowerFloorOf,
  heavenTowerGap,
  heavenTowerRows,
  MAX_FLOOR_COUNT,
} from './rows';

export interface HeavenTowerDeps {
  readonly host: QuestHost;
}

/** The id of the band layer (a control made by code). */
const ID_BAND = 0x7000;
/** `D3DCOLOR_ARGB( 60, 240, 0, 0 )`. */
const BAND_FILL = 'rgba(240, 0, 0, 0.235)';

export class WndHeavenTower extends WndNeuz {
  readonly appId = APP_HEAVEN_TOWER;
  readonly #request: HeavenTowerRequest;
  readonly #host: QuestHost;
  #chosen = -1;
  #gap = 0;
  #offset = 0;
  #band: RectLayer | null = null;

  constructor(request: HeavenTowerRequest, deps: HeavenTowerDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  /** The selected row (`m_nChoiceNum`), -1 before a pick. */
  get chosen(): number {
    return this.#chosen;
  }
  get list(): ListBox | null {
    return this.getDlgItem<ListBox>(WIDC_LISTBOX1);
  }
  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get pageButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  /** The selection band layer (tests). */
  get band(): RectLayer | null {
    return this.#band;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const { gap, offset } = heavenTowerGap(lineHeight(FONT.text));
    this.#gap = gap;
    this.#offset = offset;
    // the rows
    const list = this.list;
    const rows = heavenTowerRows((tid) => this.#host.text(tid));
    for (const r of rows) list?.addString(r);
    // the description: one page, so the page-down button goes
    const text = this.text;
    text?.setParsingString(this.#host.text(TID_TOOLTIP_HEAVENTOWER));
    const page = this.pageButton;
    page?.setVisible(false);
    page?.enableWindow(false);
    this.#layout(rows.length);
    const rect = this.rectClient;
    this.#band = this.addControl(
      new RectLayer(),
      ID_BAND,
      { l: 0, t: 0, r: rectWidth(rect), b: rectHeight(rect) },
      WBS_NOFRAME,
    );
    this.#paintBand();
    this.moveParentCenter();
  }

  /** `InitWnd`: the list, the window, the button and the text take their final sizes. */
  #layout(count: number): void {
    const listCtl = this.getWndCtrl(WIDC_LISTBOX1);
    const textCtl = this.getWndCtrl(WIDC_TEXT1);
    const buttonCtl = this.getWndCtrl(WIDC_BUTTON1);
    if (listCtl && textCtl && count <= MAX_FLOOR_COUNT) {
      const r = listCtl.rect;
      const next: Rect = {
        l: r.l,
        t: r.t,
        r: r.r + HEAVEN_TOWER_WIDEN,
        b: r.t + count * this.#gap + 10,
      };
      this.list?.setWndRect(next, true);
      const w = this.rectWindow;
      this.setWndRect(
        { l: w.l, t: w.t, r: w.r, b: w.t + (next.b - next.t) + rectHeight(textCtl.rect) + 60 },
        true,
      );
    }
    // the English texts are wider
    const w = this.rectWindow;
    this.setWndRect({ l: w.l, t: w.t, r: w.r + HEAVEN_TOWER_WIDEN, b: w.b }, true);
    if (buttonCtl) {
      const r = buttonCtl.rect;
      this.pageButton?.setWndRect(
        { l: r.l + HEAVEN_TOWER_WIDEN, t: r.t, r: r.r + HEAVEN_TOWER_WIDEN, b: r.b },
        true,
      );
    }
    if (textCtl) {
      const r = textCtl.rect;
      this.text?.setWndRect({ l: r.l, t: r.t, r: r.r + HEAVEN_TOWER_WIDEN, b: r.b }, true);
    }
  }

  /** `OnDraw`: the band over the chosen row. */
  #paintBand(): void {
    const at = this.getWndCtrl(WIDC_CUSTOM2)?.rect;
    if (this.#chosen < 0 || !at) {
      this.#band?.setEntries([]);
      return;
    }
    const top = at.t + this.#chosen * this.#gap + this.#offset;
    this.#band?.setEntries([
      {
        key: 'band',
        x: at.l,
        y: top,
        w: rectWidth(at) + HEAVEN_TOWER_WIDEN,
        h: rectHeight(at),
        fill: BAND_FILL,
      },
    ]);
  }

  /** The row `row` was clicked (`WIDC_LISTBOX1`). */
  choose(row: number): void {
    if (row < 0) return;
    this.#chosen = row;
    this.#paintBand();
    if (row === HEAVEN_TOWER_CLOSE_ROW) {
      this.destroy();
      return;
    }
    const msgTid = HEAVEN_TOWER_MSG_TIDS[row];
    if (msgTid === undefined) return;
    const text =
      row === HEAVEN_TOWER_FREE_ROW
        ? this.#host.fmt(msgTid)
        : this.#host.fmt(msgTid, HEAVEN_TOWER_COSTS[row] ?? 0);
    this.#host.confirm(
      text,
      () => {
        const floor = heavenTowerFloorOf(row);
        if (floor === null) return;
        this.#request.enter(floor);
        this.destroy();
      },
      this,
    );
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_LISTBOX1 && msg !== Wnm.SELCANCEL) {
      this.choose(this.list?.getCurSel() ?? -1);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
