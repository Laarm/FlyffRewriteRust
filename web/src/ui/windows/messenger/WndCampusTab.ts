// `WndCampusTab` (`CWndCampus`, `src:_Interface/WndMessengerCtrl.cpp:1642-2200`, Plan/42 §19.4 I; WP-UI-44, wired by 45 WP-CFEAT-40): `APP_MESSENGER_TAB_CAMPUS`
// (324 x 228, no title), the third page of the messenger. The layout has two group boxes ("Mentor", "Pupil") and two disabled list boxes; the rows are drawn over
// them like the other pages: the master of a pupil at y 43, the disciples of a master from y 122, 20 px apart, with the same columns. One menu entry,
// `TID_GAME_MENU_CAMPUS_SEVERANCE`, on the row of the other side. The shell reads `CampusTabPorts` (installed by `ui.messenger.setPorts({ campus })`); without ports the
// lists are empty.

import {
  APP_MESSENGER_TAB_CAMPUS,
  WIDC_LISTBOX_DISCIPLE,
  WIDC_LISTBOX_MASTER,
} from '../../../gen/defs/resdata';
import { TID_GAME_MENU_CAMPUS_SEVERANCE } from '../../../gen/defs/text';
import type { ListBox } from '../../controls/ListBox';
import { rectHeight, rectWidth } from '../../layout/geometry';
import type { TooltipService } from '../../tooltip/TooltipService';
import { CodePage } from '../char/CodePage';
import type { MoverMenu } from '../npc/MoverMenu';
import { MessengerList } from './MessengerList';
import {
  type CampusTabPorts,
  EMPTY_CAMPUS_VIEW,
  type CampusView,
  type MessengerRow,
} from './ports';
import { ListSort, type SortKey } from './sort';

/** `MASTER_RENDERING_POSITION` and `DISCIPLE_RENDERING_POSITION`: the y of the first row of each list. */
export const MASTER_ROW_Y = 43;
export const DISCIPLE_ROW_Y = 122;

const ID_MASTER_LIST = 0x7e51;
const ID_DISCIPLE_LIST = 0x7e52;

export interface CampusTabDeps {
  readonly ports: () => CampusTabPorts;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  /** The popup of the page (`m_Menu.CreateMenu( this )`). */
  readonly menu: MoverMenu;
}

export class WndCampusTab extends CodePage {
  override readonly appId: number = APP_MESSENGER_TAB_CAMPUS;
  readonly #deps: CampusTabDeps;
  readonly #sort = new ListSort();
  #masterList: MessengerList | null = null;
  #discipleList: MessengerList | null = null;
  #applied: CampusView | null = null;
  #raw: readonly MessengerRow[] = [];
  #shown: readonly MessengerRow[] = [];

  constructor(deps: CampusTabDeps) {
    super();
    this.#deps = deps;
  }

  /** The popup of the page (its only entry is the severance). */
  get menu(): MoverMenu {
    return this.#deps.menu;
  }
  get masterList(): MessengerList | null {
    return this.#masterList;
  }
  get discipleList(): MessengerList | null {
    return this.#discipleList;
  }
  /** The disciples in display order. */
  get disciples(): readonly MessengerRow[] {
    return this.#shown;
  }
  get sort(): ListSort {
    return this.#sort;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `pWndListBox->EnableWindow( FALSE )`: the two list boxes of the layout are frames only
    this.getDlgItem<ListBox>(WIDC_LISTBOX_MASTER)?.enableWindow(false);
    this.getDlgItem<ListBox>(WIDC_LISTBOX_DISCIPLE)?.enableWindow(false);
    const client = this.getClientRect(false);
    const width = rectWidth(client);
    const make = (id: number, top: number, bottom: number): MessengerList =>
      this.addControl(
        new MessengerList(
          {
            activate: () => undefined,
            context: (rowId, at) => {
              this.#openMenu(rowId, at);
            },
            jobTip: (name, rect) => {
              if (name === null || name === '' || rect === null) this.#deps.tooltip.cancel();
              else this.#deps.tooltip.put(this, name, rect, 3);
            },
          },
          false,
        ),
        id,
        // the first row sits at `top + 3` (the list starts its rows 3 px below its top)
        { l: 0, t: top - 3, r: width, b: bottom },
      );
    this.#masterList = make(ID_MASTER_LIST, MASTER_ROW_Y, MASTER_ROW_Y + 20);
    this.#discipleList = make(ID_DISCIPLE_LIST, DISCIPLE_ROW_Y, rectHeight(client));
    this.refresh();
  }

  override process(now: number): void {
    const v = this.#deps.ports().view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  /** `UpdatePlayerList`: reads the view again. */
  refresh(): void {
    this.#apply(this.#deps.ports().view.get());
  }

  #apply(v: CampusView): void {
    this.#applied = v;
    this.#masterList?.setMessengerRows(v.role === 'pupil' && v.master ? [v.master] : []);
    this.#raw = v.role === 'master' ? v.disciples : [];
    this.#shown = this.#sort.sortRebuild(this.#raw);
    this.#discipleList?.setMessengerRows(this.#shown);
  }

  /** A click on one of the five sort buttons of the messenger: the disciples sort (the master is one row). */
  sortBy(key: SortKey): void {
    this.#shown = this.#sort.sortClick(this.#raw, key);
    this.#discipleList?.setMessengerRows(this.#shown);
  }

  /** Whether the view is the empty one (no campus). */
  get empty(): boolean {
    return (this.#applied ?? EMPTY_CAMPUS_VIEW).role === 'none';
  }

  #openMenu(id: number, at: { x: number; y: number }): void {
    const menu = this.#deps.menu;
    menu.onPick = () => {
      this.#deps.ports().actions.severance(id);
    };
    menu.show([{ id: 0, text: this.#deps.getText(TID_GAME_MENU_CAMPUS_SEVERANCE) }], at);
  }
}
