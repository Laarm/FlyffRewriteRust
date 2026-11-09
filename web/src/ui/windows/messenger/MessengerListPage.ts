// `MessengerListPage` (`CWndFriendCtrlEx`, `CWndGuildCtrlEx`, `src:_Interface/WndMessengerCtrl.cpp:227-1640`, Plan/42 §19.4 I; WP-UI-43): a tab page of the messenger
// that holds one `MessengerList`. The friend page has the right-click menu, the guild page has none; both sort by the five buttons of the window with the
// state machine of the original (`sort.ts`) and rebuild their list from the view (`UpdatePlayerList`).

import type { Point } from '../../layout/geometry';
import type { TooltipService } from '../../tooltip/TooltipService';
import { CodePage } from '../char/CodePage';
import { ListSort, type SortKey } from './sort';
import { MessengerList, type MessengerListHooks } from './MessengerList';
import type { MessengerActions, MessengerPage, MessengerRow } from './ports';
import type { MoverMenu } from '../npc/MoverMenu';

const ID_LIST = 0x7e41;

export interface ListPageDeps {
  readonly kind: MessengerPage;
  readonly actions: MessengerActions;
  readonly tooltip: TooltipService;
  /** The popup of the friend page (`m_menu.CreateMenu( this )`); `null` for the guild page, which has no menu. */
  readonly menu: MoverMenu | null;
}

export class MessengerListPage extends CodePage {
  readonly kind: MessengerPage;
  readonly #deps: ListPageDeps;
  readonly #sort = new ListSort();
  #list: MessengerList | null = null;
  /** The rows as the view gave them. */
  #raw: readonly MessengerRow[] = [];
  /** The rows in display order. */
  #shown: readonly MessengerRow[] = [];

  constructor(deps: ListPageDeps) {
    super();
    this.#deps = deps;
    this.kind = deps.kind;
  }

  get list(): MessengerList | null {
    return this.#list;
  }
  get sort(): ListSort {
    return this.#sort;
  }
  /** The rows in display order (what the list shows). */
  get rows(): readonly MessengerRow[] {
    return this.#shown;
  }
  /** The selected row, `null` for none. */
  get selectedRow(): MessengerRow | null {
    const id = this.#list?.selectedId ?? null;
    return id === null ? null : (this.#shown.find((r) => r.id === id) ?? null);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const kind = this.kind;
    const actions = this.#deps.actions;
    const menu = this.#deps.menu;
    const hooks: MessengerListHooks = {
      activate: (id) => {
        actions.activate(kind, id);
      },
      jobTip: (name, rect) => {
        if (name === null || name === '' || rect === null) this.#deps.tooltip.cancel();
        else this.#deps.tooltip.put(this, name, rect, 3);
      },
      clearTarget: () => {
        actions.clearTarget();
      },
      ...(menu === null
        ? {}
        : {
            context: (id: number, at: Point) => {
              this.#openMenu(id, at, menu);
            },
          }),
    };
    this.#list = this.addControl(new MessengerList(hooks), ID_LIST, this.getClientRect(false));
    this.#list.setMessengerRows(this.#shown);
  }

  #openMenu(id: number, at: Point, menu: MoverMenu): void {
    const entries = this.#deps.actions.friendMenu(id);
    if (entries.length === 0) return;
    menu.onPick = (entry) => {
      this.#deps.actions.pickFriendMenu(id, entry);
    };
    menu.show(
      entries.map((e) => ({ id: e.id, text: e.text })),
      at,
    );
  }

  /** `UpdatePlayerList`: the rows of the view, sorted in the direction of the last click. */
  setRows(rows: readonly MessengerRow[]): void {
    this.#raw = rows;
    this.#shown = this.#sort.sortRebuild(rows);
    this.#list?.setMessengerRows(this.#shown);
  }

  /** A click on one of the five sort buttons. */
  sortBy(key: SortKey): void {
    this.#shown = this.#sort.sortClick(this.#raw, key);
    this.#list?.setMessengerRows(this.#shown);
  }
}
