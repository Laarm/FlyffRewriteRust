// `WndVendorChat` (`CWndVendorMessage`, `src:_Interface/WndVendor.cpp:430-640`, ref/22 §17 F, Plan/42 §19.4 F, 45 §15.3; WP-UI-37):
// `APP_VENDOREX_CHAT` (448 x 320), the chat room of a personal shop: the member list on the right (`WIDC_LISTBOX1`, "User"), a tab control on the left
// with the page "Talk" (every line said in the room) and, for the SHOP OWNER only, the page "Info" (who came in, who left, what was sold), the input
// edit (`WIDC_EDIT1`) and the Send button (`WIDC_BUTTON1`). Enter in the edit or the button sends the line and empties the edit; an empty line sends
// nothing. Closing the window with its own button tells the feature (the owner closes the room: `SendCloseChattingRoom`).
//
// The pages follow the view: the lists grow, so only the new lines are appended and the pages scroll to the end (`m_wndScrollBar.SetMaxScrollPos()`).

import {
  APP_VENDOREX_CHAT,
  WIDC_BUTTON1,
  WIDC_EDIT1,
  WIDC_LISTBOX1,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import { TID_APP_DIALOG, TID_APP_INFOMATION } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { ListBox } from '../../controls/ListBox';
import type { TabCtrl } from '../../controls/TabCtrl';
import { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rect } from '../../layout/geometry';
import { WBS_VSCROLL, WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CodePage } from '../char/CodePage';
import { CHAT_LINE_MAX } from './constants';
import type { VendorChatActions, VendorChatView, ViewSource } from './ports';

/** The window ids of the two pages (`m_wndChat.Create( .., pWndTabCtrl, 10 )`, `m_wndInfo` 11). */
export const WIDC_TALK_PAGE = 10;
export const WIDC_INFO_PAGE = 11;

export interface VendorChatDeps {
  readonly view: ViewSource<VendorChatView>;
  readonly actions: VendorChatActions;
  readonly getText: (tid: number) => string;
}

/** A tab page that is one scrolling rich text (`CWndText` with `WBS_VSCROLL`). */
class TextPage extends CodePage {
  text: TextCtrl | null = null;
  /** How many lines of the view the page shows already. */
  shown = 0;

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.getClientRect(false);
    this.text = this.addControl(new TextCtrl(), 1, rect(0, 0, r.r - r.l, r.b - r.t), WBS_VSCROLL);
  }

  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    this.text?.setWndRect(rect(0, 0, cx, cy));
  }
}

export class WndVendorChat extends WndNeuz {
  readonly appId = APP_VENDOREX_CHAT;
  readonly #deps: VendorChatDeps;
  #tab: TabCtrl | null = null;
  #talk: TextPage | null = null;
  #info: TextPage | null = null;
  #members: ListBox | null = null;
  #edit: EditCtrl | null = null;
  #applied: VendorChatView | null = null;
  #shownMembers: readonly string[] = [];
  #off: (() => void) | null = null;
  #closedNotified = false;
  #byUser = false;

  constructor(deps: VendorChatDeps) {
    super();
    this.#deps = deps;
  }

  // ---- accessors for tests and the feature layer -----------------------------------------------------------------------

  get talkPage(): TextCtrl | null {
    return this.#talk?.text ?? null;
  }
  get infoPage(): TextCtrl | null {
    return this.#info?.text ?? null;
  }
  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get memberList(): ListBox | null {
    return this.#members;
  }
  get input(): EditCtrl | null {
    return this.#edit;
  }
  get sendButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }

  // ---- OnInitialUpdate -------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#members = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    this.#tab = tab;
    const owner = this.#deps.view.get().owner;
    if (tab) {
      const talk = new TextPage();
      talk.attach(tab, WIDC_TALK_PAGE);
      this.#talk = talk;
      tab.insertItem(0, { text: this.#deps.getText(TID_APP_DIALOG), page: talk });
      if (owner) {
        const info = new TextPage();
        info.attach(tab, WIDC_INFO_PAGE);
        this.#info = info;
        tab.insertItem(1, { text: this.#deps.getText(TID_APP_INFOMATION), page: info });
      }
      tab.setCurSel(0);
    }
    this.#edit?.setFocus();
    // the window opens at the right edge, 110 px from the top, then centres (`Move`, `MoveParentCenter`)
    this.moveParentCenter();
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  // ---- the view --------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #grow(page: TextPage | null, lines: readonly string[]): void {
    const text = page?.text;
    if (!page || !text) return;
    if (lines.length < page.shown) {
      // the room was reset: start the page again
      text.setString('');
      page.shown = 0;
    }
    if (lines.length === page.shown) return;
    text.addStrings(lines.slice(page.shown).map((t) => ({ text: t, color: 0xff000000 })));
    page.shown = lines.length;
    text.scrollBar?.setMaxScrollPos();
  }

  #apply(v: VendorChatView): void {
    this.#applied = v;
    this.#grow(this.#talk, v.talk);
    this.#grow(this.#info, v.info);
    const list = this.#members;
    if (list && !sameNames(this.#shownMembers, v.members)) {
      list.resetContent();
      for (const n of v.members) list.addString(n);
      this.#shownMembers = v.members;
    }
  }

  // ---- sending ---------------------------------------------------------------------------------------------------------------

  /** Sends the line of the edit and empties it (`strlen( .. ) < 1020`; an empty line sends nothing). */
  send(): boolean {
    const edit = this.#edit;
    if (!edit) return false;
    const text = edit.getString();
    if (text === '' || text.length > CHAT_LINE_MAX) return false;
    this.#deps.actions.send(text);
    edit.empty();
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_EDIT1 && msg === Wnm.EN_RETURN) {
      this.send();
      return true;
    }
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.send();
        return true;
      }
      if (id === WTBID_CLOSE) {
        // the owner closes the room with the window (`if( IsVendorOpen() ) SendCloseChattingRoom()`)
        this.#byUser = true;
        this.#deps.actions.closedByUser();
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** The window was closed with its own title-bar button. */
  get closedByUser(): boolean {
    return this.#byUser;
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    if (!this.#closedNotified) {
      this.#closedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}

function sameNames(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((n, i) => n === b[i]);
}
