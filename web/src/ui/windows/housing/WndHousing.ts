// `WndHousing` (`CWndHousing`, `src:_Interface/WndHousing.cpp:36-551`, Plan/42 §19.4 N, 45 §28.10; WP-UI-58): `APP_HOUSING` (448 x 416), the furniture list of the own mini room. An APPLET (the start menu row exists in the full
// menu only, `features.clean_start_menu = false`).
//
//   * ten visible rows (`HouseList`, pitch `font + 1`): the type text (x 5, `TID_GAME_HOUSING_*` by `IK3_*`), the name (85), the remaining time (250: under an hour `TID_GAME_TIME_1HOUR`, else days and hours) and `O` / `X` (375);
//     the selected row is blue, the others black; the layout's list box stays under the rows for the frame;
//   * the headers `STATIC2..5` sort on a click (the same column flips, a new one starts "greater"; a STABLE sort, FIX); EVERY refresh resets the selection and disables both buttons;
//   * selecting a row shows its description in `TEXT1` and enables `BUTTON1` (install, not installed) or `BUTTON2` (remove, installed) only in the mini room of the player (`port.canEdit()`, read at the selection);
//   * `BUTTON1` ends a running placement first; a wallpaper or a carpet is requested at once, a furniture starts the placement and the world takes the focus; `BUTTON2` (not while placing) requests the removal;
//   * the window swallows the wheel (`OnMouseWheel` returns TRUE).

import {
  APP_HOUSING,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_LISTBOX1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { IK3_CARPET, IK3_WALLPAPER } from '../../../gen/defs/itemkind';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import { type Point, rectContains, rectHeight, rectWidth } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import { FONT, lineHeight } from '../../text/fonts';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { HouseList, type HouseRow } from './HouseList';
import type { HousingHost, HousingPort } from './ports';
import {
  COLOR_BLACK,
  COLOR_SELECTED_BLUE,
  type HousingColumn,
  type HousingItemView,
  HOUSING_PAGE_ROWS,
  housingTimeText,
  housingTypeTid,
  nextSortState,
  sortHousing,
} from './rows';

/** The id of the row list (a control made by code, over the layout's list box). */
export const HOUSING_LIST_ID = 0x7a01;

const HEADERS: readonly { readonly id: number; readonly column: HousingColumn }[] = [
  { id: WIDC_STATIC2, column: 'type' },
  { id: WIDC_STATIC3, column: 'name' },
  { id: WIDC_STATIC4, column: 'time' },
  { id: WIDC_STATIC5, column: 'installed' },
];

export interface HousingDeps {
  readonly port: () => HousingPort | null;
  readonly host: HousingHost;
}

export class WndHousing extends WndNeuz {
  readonly appId = APP_HOUSING;
  readonly #deps: HousingDeps;
  #list: HouseList | null = null;
  /** `m_nSortType` / `m_bIsGreater`: the type column, greater. */
  #sort: { column: HousingColumn | null; greater: boolean } = { column: 'type', greater: true };
  #shown: readonly HousingItemView[] = [];
  #selected = -1;

  constructor(deps: HousingDeps) {
    super();
    this.#deps = deps;
  }

  get list(): HouseList | null {
    return this.#list;
  }
  get installButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get removeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get description(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  /** The items in display order (`m_mapItem`). */
  get items(): readonly HousingItemView[] {
    return this.#shown;
  }
  /** The sort column and direction (`m_nSortType`, `m_bIsGreater`). */
  get sort(): { column: HousingColumn | null; greater: boolean } {
    return { ...this.#sort };
  }
  /** The selected row (`m_nSelected - 1`), -1 for none. */
  get selected(): number {
    return this.#selected;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const frame = this.getWndCtrl(WIDC_LISTBOX1);
    if (frame) {
      const r = frame.rect;
      const font = lineHeight(FONT.text);
      this.#list = this.addControl(
        new HouseList(
          { pitch: font + 1, top: 8, page: HOUSING_PAGE_ROWS },
          {
            select: (_row, index) => {
              this.#onSelect(index);
            },
          },
        ),
        HOUSING_LIST_ID,
        { l: r.l, t: r.t, r: r.r, b: r.t + rectHeight(r) },
      );
    }
    this.#refresh();
    this.moveParentCenter();
  }

  // ---- RefreshItemList ---------------------------------------------------------------------------------------------------

  /** `RefreshItemList()`: the list is rebuilt from the port, sorted, the selection resets and both buttons are disabled. */
  refresh(): void {
    this.#refresh();
  }

  #refresh(): void {
    const port = this.#deps.port();
    this.#shown = sortHousing(port?.items() ?? [], this.#sort.column ?? 'type', this.#sort.greater);
    this.#selected = -1;
    this.#rebuildRows(null);
    this.#setButtons(false, false);
    this.description?.setString('');
  }

  /** Pushes the rows; `select` is the key to keep selected (`undefined`: the current one), `null` clears the selection. */
  #rebuildRows(select?: number | null): void {
    const port = this.#deps.port();
    const now = port?.nowSec() ?? 0;
    const t = this.#deps.host.text;
    const rows: HouseRow[] = this.#shown.map((item, i) => {
      const tid = housingTypeTid(item.kind3);
      return {
        key: i,
        color: COLOR_BLACK,
        selectedColor: COLOR_SELECTED_BLUE,
        cells: [
          ...(tid === null ? [] : [{ x: 5, y: 0, text: t(tid) }]),
          { x: 85, y: 0, text: item.name },
          { x: 250, y: 0, text: housingTimeText(item.expireAt, now, t) },
          { x: 375, y: 0, text: item.installed ? 'O' : 'X' },
        ],
      };
    });
    this.#list?.setRows(rows, select);
  }

  /** The time column changes with the clock: the rows are redrawn at the UI rate (an unchanged list emits nothing). */
  override process(now: number): void {
    super.process(now);
    if (this.#shown.length > 0) this.#rebuildRows();
  }

  #setButtons(install: boolean, remove: boolean): void {
    this.installButton?.enableWindow(install);
    this.removeButton?.enableWindow(remove);
  }

  // ---- the list ------------------------------------------------------------------------------------------------------------

  /** `OnChildNotify( WIDC_LISTBOX1 )`: the description and the button of the selected row. */
  #onSelect(index: number): void {
    this.#selected = index;
    const item = this.#shown[index];
    if (!item) return;
    this.description?.setString(item.description);
    this.#setButtons(false, false);
    const edit = this.#deps.port()?.canEdit() === true;
    if (edit) {
      if (item.installed) this.#setButtons(false, true);
      else this.#setButtons(true, false);
    }
  }

  /** The selected item, `null` for none. */
  get selectedItem(): HousingItemView | null {
    return this.#shown[this.#selected] ?? null;
  }

  // ---- the headers ---------------------------------------------------------------------------------------------------------

  /** `OnLButtonUp`: a release over a header sorts by its column. */
  override onLButtonUp(pt: Point): void {
    for (const h of HEADERS) {
      const c = this.getWndCtrl(h.id);
      if (!c || !rectContains(c.rect, pt)) continue;
      this.#sort = nextSortState(this.#sort, h.column);
      this.#refresh();
    }
  }

  /** Sorts by a column as a click on its header does. */
  sortBy(column: HousingColumn): void {
    this.#sort = nextSortState(this.#sort, column);
    this.#refresh();
  }

  // ---- the buttons ---------------------------------------------------------------------------------------------------------

  /** `WIDC_BUTTON1`: an active placement ends first; a paper is requested at once, a furniture starts the placement. */
  install(): void {
    const port = this.#deps.port();
    const item = this.selectedItem;
    if (!port || !item) return;
    if (port.deploying()) port.endDeploy();
    if (item.kind3 === IK3_WALLPAPER || item.kind3 === IK3_CARPET) port.installPaper(item.itemId);
    else port.deploy(item.itemId);
    this.#setButtons(false, false);
  }

  /** `WIDC_BUTTON2`: the removal request (not while a placement runs). */
  remove(): void {
    const port = this.#deps.port();
    const item = this.selectedItem;
    if (!port || !item || port.deploying()) return;
    port.remove(item.itemId);
    this.#setButtons(false, false);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.install();
        return true;
      }
      if (id === WIDC_BUTTON2) {
        this.remove();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `OnMouseWheel` returns TRUE: the window swallows the wheel (the list scrolls itself when the pointer is over it). */
  override onMouseWheel(): void {
    // consumed: nothing to do
  }

  /** The list rect width (tests). */
  get listWidth(): number {
    const c = this.getWndCtrl(WIDC_LISTBOX1);
    return c ? rectWidth(c.rect) : 0;
  }
}
