// `WndRoomList` (`CWndRoomList`, `src:_Interface/WndRoomList.cpp:39-135`, Plan/42 §19.4 N, 45 §28.10; WP-UI-58): `APP_MINIROOM_LIST` (272 x 272), the rooms of the friends the player may visit. Opening it clears the visitable list
// and asks the server for it (`request.opened()`); the rows read `"%d. %s"` over the owners whose NAME is known (KEEP: an owner without a cached name has no row; FIX: the feature asks the cache for the names and the window
// is refreshed when they arrive). The selection is red (`0xffff0000`). `BUTTON1` with a selection visits the room and closes (a double click on a row does the same), `BUTTON2` asks for the list again, `BUTTON3` closes.

import {
  APP_MINIROOM_LIST,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_LISTBOX1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { ListBox } from '../../controls/ListBox';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { RoomListRequest } from './ports';
import { COLOR_ROOM_SELECT, roomListRows } from './rows';

export class WndRoomList extends WndNeuz {
  readonly appId = APP_MINIROOM_LIST;
  readonly #request: RoomListRequest;

  constructor(request: RoomListRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get list(): ListBox | null {
    return this.getDlgItem<ListBox>(WIDC_LISTBOX1);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get refreshButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON3);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.list?.setSelectColor(COLOR_ROOM_SELECT);
    this.#request.opened();
    this.refresh();
    this.moveParentCenter();
  }

  /** `Refresh()`: the list box is rebuilt from the owners with a known name (the item data is the character id). */
  refresh(): void {
    const list = this.list;
    if (!list) return;
    const sel = list.getCurSel() >= 0 ? list.getItemData(list.getCurSel()) : -1;
    list.resetContent();
    for (const row of roomListRows(this.#request.owners(), (id) => this.#request.nameOf(id))) {
      const i = list.addString(row.text);
      list.setItemData(i, row.id);
      if (row.id === sel) list.setCurSel(i);
    }
  }

  /** The visible texts (tests). */
  get rows(): string[] {
    const list = this.list;
    if (!list) return [];
    return Array.from({ length: list.getCount() }, (_, i) => list.getPlainText(i));
  }

  /** `BUTTON1`: visits the selected room and closes. */
  visit(): void {
    const list = this.list;
    if (!list || list.getCurSel() < 0) return;
    this.#request.visit(list.getItemData(list.getCurSel()));
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_LISTBOX1 && msg === Wnm.DBLCLK) {
      this.visit();
      return true;
    }
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.visit();
        return true;
      }
      if (id === WIDC_BUTTON2) {
        this.#request.refresh();
        return true;
      }
      if (id === WIDC_BUTTON3) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
