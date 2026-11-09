// `WndBlockingList` (`CWndChattingBlockingList`, `src:_Interface/WndChattingBlockingList.cpp`, Plan/42 §19.4 I, 23 §6.11; WP-UI-44): `APP_CHATTING_BLOCKING_LIST` (224 x 256),
// the ignore list of `/ignorelist`, READ ONLY as in the original (the list box cannot be edited, there are no buttons). The title is `[ <own name> ]'s block list`
// (`TID_GAME_CHATTING_BLOCKING_LIST_TITLE`), the group box says how many are listed (`..._TOTAL_NUMBER`), the list box holds the names in stored order. The window follows
// its view: a new `ChatSettingsNtf` changes the list while the window is open. It is centred.

import {
  APP_CHATTING_BLOCKING_LIST,
  WIDC_GROUP_BOX_TOTAL_NUMBER,
  WIDC_LISTBOX_BLOCKING_USERS,
} from '../../../gen/defs/resdata';
import type { ListBox } from '../../controls/ListBox';
import type { Static } from '../../controls/Static';
import { WndNeuz } from '../../wnd/WndNeuz';
import { type BlockListRequest, type BlockListView } from './ports';

export class WndBlockingList extends WndNeuz {
  readonly appId = APP_CHATTING_BLOCKING_LIST;
  readonly #request: BlockListRequest;
  #list: ListBox | null = null;
  #group: Static | null = null;
  #unsubscribe: (() => void) | null = null;
  #applied: BlockListView | null = null;

  constructor(request: BlockListRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get list(): ListBox | null {
    return this.#list;
  }
  get group(): Static | null {
    return this.#group;
  }
  /** The names in the list box, in order. */
  get names(): string[] {
    const list = this.#list;
    if (!list) return [];
    return Array.from({ length: list.getCount() }, (_, i) => list.getPlainText(i));
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#group = this.getDlgItem<Static>(WIDC_GROUP_BOX_TOTAL_NUMBER);
    this.#list = this.getDlgItem<ListBox>(WIDC_LISTBOX_BLOCKING_USERS);
    this.#apply(this.#request.view.get());
    this.#unsubscribe = this.#request.view.subscribe(() => {
      this.#apply(this.#request.view.get());
    });
    this.moveParentCenter();
  }

  /** `UpdateInformation`: the title, the group caption and the names. */
  #apply(v: BlockListView): void {
    if (v === this.#applied) return;
    this.#applied = v;
    this.setTitle(v.title);
    this.#group?.setTitle(v.total);
    const list = this.#list;
    if (!list) return;
    list.resetContent();
    for (const name of v.names) list.addString(name);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
