// `WndPartyFind` (`CWndPartyFind`, `src:_Interface/WndField.cpp:27864-27965`, ref/22 §17 S, Plan/42 §19.4 S, 45 §22.3; WP-UI-40, `features.party_find`):
// `APP_PARTY_FIND` (608 x 384, hotkey U), the party finder. Opening it asks the server for the recruiting parties; the list shows each party's
// name, leader, size, level, points and the modes that run. A press on a row shows the member names in the text area under the list; Refresh
// clears the list and asks again; Join asks to join the selected party and closes the window. The two header statics `WIDC_STATIC51` and
// `WIDC_STATIC52` have no id in `ResData.h` and are reached by name. The layout's list box is replaced by `FinderList`.

import {
  APP_PARTY_FIND,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_LISTBOX1,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { FinderList } from './FinderList';
import type { FinderActions, FinderRow, FinderView, ViewSource } from './ports';

export interface FinderDeps {
  readonly view: ViewSource<FinderView>;
  readonly actions: FinderActions;
}

export class WndPartyFind extends WndNeuz {
  readonly appId = APP_PARTY_FIND;
  readonly #deps: FinderDeps;
  #list: FinderList | null = null;
  #applied: FinderView | null = null;
  #unsubscribe: (() => void) | null = null;

  constructor(deps: FinderDeps) {
    super();
    this.#deps = deps;
  }

  get list(): FinderList | null {
    return this.#list;
  }
  get refreshButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get joinButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get members(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  /** `WIDC_STATIC51` ("#/8") and `WIDC_STATIC52` ("Scrolls / Skills"): no id in `ResData.h`, found by name. */
  get sizeHeader(): Static | null {
    return this.getDlgItemByName<Static>('WIDC_STATIC51');
  }
  get flagsHeader(): Static | null {
    return this.getDlgItemByName<Static>('WIDC_STATIC52');
  }
  get selected(): FinderRow | null {
    return this.#list?.selectedFinderRow ?? null;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // the layout's list box gives the rect; the party list is drawn by the finder list in its place
    const box = this.getDlgItem(WIDC_LISTBOX1);
    const rect = this.getWndCtrl(WIDC_LISTBOX1)?.rect;
    box?.setVisible(false);
    if (rect) {
      this.#list = this.addControl(
        new FinderList({
          select: (row) => {
            this.members?.setString(row?.memberList ?? '');
          },
        }),
        0x7d51,
        rect,
      );
    }
    this.#apply(this.#deps.view.get());
    this.#unsubscribe = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    // `g_DPlay.JoinParty()`
    this.#deps.actions.refresh();
    this.moveParentCenter();
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #apply(v: FinderView): void {
    this.#applied = v;
    this.#list?.setRows(v.rows, v.flagIcons);
    this.members?.setString('');
  }

  /** Refresh: the list is cleared and asked for again. */
  refresh(): void {
    this.#list?.clear();
    this.members?.setString('');
    this.#deps.actions.refresh();
  }

  /** Join: the selected party's leader (nothing for no selection); the window closes. */
  join(): void {
    const row = this.selected;
    if (!row) return;
    this.#deps.actions.join(row.leaderId);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.refresh();
        return true;
      }
      if (id === WIDC_BUTTON2) {
        this.join();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    super.onDestroy();
  }
}
