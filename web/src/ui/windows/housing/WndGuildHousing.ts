// `WndGuildHousing` (`CWndGuildHousing`, `src:_Interface/WndHousing.cpp:555-1380`, Plan/42 §19.4 N, 45 §28.10; WP-UI-58): `APP_GH_FURNITURE_STORAGE` (448 x 512), the furniture and teleporter storage of the guild house. An APPLET.
//
//   * two sections: furniture (title `TID_GAME_GUILDHOUSE_LOCKER`, the combo of eleven kinds, skips `IK2_GUILDHOUSE_NPC`) and teleporter (title `TID_GAME_GUILDHOUSE_TELEPORTER`, the combo disabled with the one entry "TELEPORTER",
//     skips furniture and papering); `initBySection` switches;
//   * seven rows of 40 px (`HouseList`): the icon at (+16, +10), the name at (+80, +20), the time at (+260, +20) (days, hours or minutes of the largest unit, `(NEW)` while the item never ran), installed rows blue, the selected row a red
//     fill `0x55ff0000`; `TEXT1` shows the description of the selected row;
//   * the headers `STATIC1..3` sort (name, time, installed; a STABLE sort, the same column flips, a new one starts "greater") with `LvUp.bmp` / `LvDn.bmp` at `(left - 18, top)`; the list starts in list order without a mark;
//   * the seven row buttons (x 368..408, 40 high): visible for rows with data, `Buttlockerinstall.bmp` + `TID_TOOLTIP_GUILDHOUSE_BUTT_DISMANTLE` for an installed row else `Buttlockerdismantle.bmp` + `_BUTT_INSTALL` (KEEP the
//     cross-over of the original), enabled only with the furniture power inside a guild-house world; in the teleporter section the not installed ones are disabled while one is installed (FIX: only the visible buttons, scroll aware);
//   * a click on a row selects its object id (`port.select`); a refresh re-selects the row of `selectedObjId` and scrolls it into view (`FixScrollBar`), keeping the scroll position otherwise;
//   * while a SETUP placement runs (`port.deployingRow()`) the window hides and `WndGHouseShowOneUnit` shows the item; when it ends both revert.

import {
  APP_GH_FURNITURE_STORAGE,
  WIDC_COMBOBOX1,
  WIDC_LISTBOX1,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDHOUSE_LOCKER,
  TID_GAME_GUILDHOUSE_TELEPORTER,
  TID_TOOLTIP_GUILDHOUSE_BUTT_DISMANTLE,
  TID_TOOLTIP_GUILDHOUSE_BUTT_INSTALL,
} from '../../../gen/defs/text';
import { Button } from '../../controls/Button';
import type { ComboBox } from '../../controls/ComboBox';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectWidth } from '../../layout/geometry';
import { EBS_READONLY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { HouseList, type HouseRow } from './HouseList';
import type { HousingHost, StoragePort } from './ports';
import {
  COLOR_BLACK,
  COLOR_SELECTED_BLUE,
  FILL_RED,
  filterGhRows,
  GH_BUTTON_FREE,
  GH_BUTTON_INSTALLED,
  GH_COMBO_FURNITURE,
  GH_COMBO_TELEPORTER,
  GH_MAX_VIEW_CAPACITY,
  GH_ROW_PITCH,
  type GhColumn,
  type GhRowView,
  type GhSection,
  ghTimeText,
  nextSortState,
  NULL_OBJ_ID,
  sortGhRows,
} from './rows';

/** The id of the row list and the base id of the seven row buttons (controls made by code). */
export const STORAGE_LIST_ID = 0x7a10;
export const STORAGE_BUTTON_ID = 0x7a20;
const ID_MARK = 0x7a30;

const HEADERS: readonly { readonly id: number; readonly column: GhColumn }[] = [
  { id: WIDC_STATIC1, column: 'name' },
  { id: WIDC_STATIC2, column: 'time' },
  { id: WIDC_STATIC3, column: 'installed' },
];

export interface StorageDeps {
  readonly port: () => StoragePort | null;
  readonly host: HousingHost;
  /** The SHOWONE window of the service. */
  readonly showOne: { open(row: GhRowView): void; close(): void };
}

export class WndGuildHousing extends WndNeuz {
  readonly appId = APP_GH_FURNITURE_STORAGE;
  readonly #deps: StorageDeps;
  #section: GhSection = 'furniture';
  #combo: ComboBox | null = null;
  #comboKind3 = 0;
  #list: HouseList | null = null;
  #marks: SpriteLayer | null = null;
  readonly #buttons: Button[] = [];
  /** `m_nSortType` (none at the start) / `m_bIsGreater`. */
  #sort: { column: GhColumn | null; greater: boolean } = { column: null, greater: true };
  #shown: readonly GhRowView[] = [];
  #deploying = false;

  constructor(deps: StorageDeps) {
    super();
    this.#deps = deps;
  }

  get list(): HouseList | null {
    return this.#list;
  }
  get combo(): ComboBox | null {
    return this.#combo;
  }
  get description(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  /** The row buttons (7). */
  get rowButtons(): readonly Button[] {
    return this.#buttons;
  }
  /** The rows in display order after the section filter, the combo filter and the sort. */
  get rows(): readonly GhRowView[] {
    return this.#shown;
  }
  get section(): GhSection {
    return this.#section;
  }
  isSection(section: GhSection): boolean {
    return this.#section === section;
  }
  /** `m_dwComboCurrIK3`. */
  get comboKind3(): number {
    return this.#comboKind3;
  }
  get sort(): { column: GhColumn | null; greater: boolean } {
    return { ...this.#sort };
  }
  /** The window hides while a placement runs. */
  get deploying(): boolean {
    return this.#deploying;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#combo = this.getDlgItem<ComboBox>(WIDC_COMBOBOX1);
    this.#combo?.addWndStyle(EBS_READONLY);
    // the combo of the furniture section; `InitBySection` fills it again
    this.#fillCombo();
    const frame = this.getWndCtrl(WIDC_LISTBOX1);
    if (frame) {
      this.#list = this.addControl(
        new HouseList(
          {
            pitch: GH_ROW_PITCH,
            top: 0,
            page: GH_MAX_VIEW_CAPACITY,
            selectFill: { color: FILL_RED, yOffset: 4, height: GH_ROW_PITCH, left: 5, right: 28 },
          },
          {
            select: (_row, index) => {
              this.#onSelect(index);
            },
          },
        ),
        STORAGE_LIST_ID,
        frame.rect,
      );
    }
    // the seven row buttons: `rect( 368, 55 + 30 i + 10 ( i + 1 ), 408, + 40 )`
    for (let i = 0; i < GH_MAX_VIEW_CAPACITY; i += 1) {
      const top = 55 + 30 * i + 10 * (i + 1);
      const b = new Button();
      this.addControl(b, STORAGE_BUTTON_ID + i, { l: 368, t: top, r: 408, b: top + 40 });
      b.setVisible(false);
      this.#buttons.push(b);
    }
    this.#marks = this.addControl(new SpriteLayer(), ID_MARK, this.getClientRect(false));
    this.getDlgItem<Static>(WIDC_STATIC4)?.setVisible(false);
    this.getDlgItem<Static>(WIDC_STATIC5)?.setVisible(false);
    this.refresh();
    this.moveParentCenter();
  }

  // ---- sections and the combo ---------------------------------------------------------------------------------------------

  /** `InitBySection`: the title, the combo (disabled in the teleporter section), `m_dwComboCurrIK3 = 0` and a refresh. */
  initBySection(section: GhSection): void {
    this.#section = section;
    this.#comboKind3 = 0;
    const t = this.#deps.host.text;
    this.setTitle(
      t(section === 'furniture' ? TID_GAME_GUILDHOUSE_LOCKER : TID_GAME_GUILDHOUSE_TELEPORTER),
    );
    this.#combo?.enableWindow(section === 'furniture');
    this.#fillCombo();
    this.refresh();
  }

  #fillCombo(): void {
    const combo = this.#combo;
    if (!combo) return;
    const t = this.#deps.host.text;
    combo.resetContent();
    if (this.#section === 'furniture') {
      for (const e of GH_COMBO_FURNITURE) {
        const i = combo.addString(t(e.tid));
        combo.setItemData(i, e.kind3);
      }
    } else {
      const i = combo.addString(GH_COMBO_TELEPORTER);
      combo.setItemData(i, 0);
    }
    combo.setCurSel(0);
  }

  // ---- RefreshItemList ----------------------------------------------------------------------------------------------------

  /**
   * `RefreshItemList()`: rows of the section and the combo, sorted, the scroll position kept, the selection re-found by `selectedObjId`, the buttons updated. The rows are rebuilt whenever the port's rows,
   * the clock or the sort changed (an identical list emits nothing).
   */
  refresh(): void {
    const port = this.#deps.port();
    const rows = filterGhRows(port?.rows() ?? [], this.#section, this.#comboKind3);
    this.#shown = this.#sort.column
      ? sortGhRows(rows, this.#sort.column, this.#sort.greater)
      : rows;
    const selObj = port?.selectedObjId() ?? null;
    const selected = selObj === null ? null : this.#shown.findIndex((r) => r.ctrlObjId === selObj);
    this.#rebuildRows(selected === null || selected < 0 ? null : selected);
    if (selected !== null && selected >= 0) this.#list?.ensureVisible(selected);
    this.#updateMarks();
    this.#updateButtons();
    this.#updateDescription();
  }

  #rebuildRows(select?: number | null): void {
    const port = this.#deps.port();
    const now = port?.nowSec() ?? 0;
    const t = this.#deps.host.text;
    const rows: HouseRow[] = this.#shown.map((r, i) => ({
      key: i,
      color: r.installed ? COLOR_SELECTED_BLUE : COLOR_BLACK,
      icon: { path: r.icon === '' ? null : r.icon, x: 16, y: 10 },
      cells: [
        { x: 80, y: 20, text: r.name },
        { x: 260, y: 20, text: ghTimeText(r, now, t) },
      ],
    }));
    this.#list?.setRows(rows, select);
  }

  #onSelect(index: number): void {
    const row = this.#shown[index];
    if (!row) return;
    this.#deps.port()?.select(row.ctrlObjId === NULL_OBJ_ID ? null : row.ctrlObjId);
    this.#updateDescription();
  }

  /** The row of the selection. */
  get selectedRow(): GhRowView | null {
    const i = this.#list?.selectedRow ?? -1;
    return this.#shown[i] ?? null;
  }

  #updateDescription(): void {
    this.description?.setString(this.selectedRow?.description ?? '');
  }

  // ---- the row buttons ------------------------------------------------------------------------------------------------------

  /** `UpdateIRButton`: visibility, texture, tooltip and enable state of the seven buttons for the rows at the scroll position. */
  #updateButtons(): void {
    const port = this.#deps.port();
    const t = this.#deps.host.text;
    const scroll = Math.max(0, this.#list?.scrollRow ?? 0);
    const manage = port?.canManage() === true;
    const installedTeleporter =
      this.#section === 'teleporter' && this.#shown.some((r) => r.installed);
    this.#buttons.forEach((b, i) => {
      const row = this.#shown[scroll + i];
      if (!row) {
        b.setVisible(false);
        return;
      }
      b.setVisible(true);
      b.setTexture(row.installed ? GH_BUTTON_INSTALLED : GH_BUTTON_FREE);
      b.setToolTip(
        t(
          row.installed
            ? TID_TOOLTIP_GUILDHOUSE_BUTT_DISMANTLE
            : TID_TOOLTIP_GUILDHOUSE_BUTT_INSTALL,
        ),
      );
      b.enableWindow(manage && !(installedTeleporter && !row.installed));
    });
  }

  /** The row that button `i` stands for (scroll aware), `null` for none. */
  rowOfButton(i: number): GhRowView | null {
    return this.#shown[Math.max(0, this.#list?.scrollRow ?? 0) + i] ?? null;
  }

  // ---- the sort headers -------------------------------------------------------------------------------------------------------

  #headerRect(id: number): Rect | null {
    return this.getWndCtrl(id)?.rect ?? null;
  }

  /** The `LvUp.bmp` / `LvDn.bmp` mark at `(left - 18, top)` of the sorted header. */
  #updateMarks(): void {
    const entries: SpriteEntry[] = [];
    const col = this.#sort.column;
    const h = HEADERS.find((e) => e.column === col);
    const rect = h ? this.#headerRect(h.id) : null;
    if (h && rect) {
      entries.push({
        key: 'sort',
        path: this.#sort.greater ? 'LvUp.bmp' : 'LvDn.bmp',
        x: rect.l - 18,
        y: rect.t,
        w: 16,
        h: 16,
      });
    }
    this.#marks?.setEntries(entries);
  }

  /** `OnLButtonUp`: a release over a header sorts by its column. */
  override onLButtonUp(pt: Point): void {
    for (const h of HEADERS) {
      const r = this.#headerRect(h.id);
      if (!r || !rectContains(r, pt)) continue;
      this.sortBy(h.column);
    }
  }

  sortBy(column: GhColumn): void {
    this.#sort = nextSortState(this.#sort, column);
    this.refresh();
  }

  // ---- notifications ------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id >= STORAGE_BUTTON_ID && id < STORAGE_BUTTON_ID + GH_MAX_VIEW_CAPACITY) {
      if (msg === Wnm.CLICKED) {
        this.pressRowButton(id - STORAGE_BUTTON_ID);
        return true;
      }
    }
    if (id === WIDC_COMBOBOX1 && msg === Wnm.SELCHANGE) {
      const combo = this.#combo;
      if (combo) {
        this.#comboKind3 = combo.getSelectedItemData();
        // a combo change goes back to the top of the list
        this.#list?.scrollTo(0);
        this.refresh();
      }
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** A row button: the feature decides (`port.rowButton`). */
  pressRowButton(i: number): void {
    const row = this.rowOfButton(i);
    if (row) this.#deps.port()?.rowButton(row);
  }

  // ---- the placement ----------------------------------------------------------------------------------------------------------

  /**
   * `Process`: a running SETUP placement hides the window and shows the item in `WndGHouseShowOneUnit`; when it ends both revert. Hidden frames are not processed by the manager,
   * so the feature also calls this through `ui.housing.syncDeploy()`.
   */
  syncDeploy(): void {
    const row = this.#deps.port()?.deployingRow() ?? null;
    if (row !== null && !this.#deploying) {
      this.#deploying = true;
      this.setVisible(false);
      this.#deps.showOne.open(row);
    } else if (row === null && this.#deploying) {
      this.#deploying = false;
      this.setVisible(true);
      this.#deps.showOne.close();
    }
  }

  override process(now: number): void {
    super.process(now);
    this.syncDeploy();
    this.#rebuildRows();
    this.#updateButtons();
  }

  /** The width of the list (tests). */
  get listWidth(): number {
    const c = this.getWndCtrl(WIDC_LISTBOX1);
    return c ? rectWidth(c.rect) : 0;
  }

  override onDestroy(): void {
    this.#deps.showOne.close();
    this.#deps.port()?.closed();
    super.onDestroy();
  }
}
