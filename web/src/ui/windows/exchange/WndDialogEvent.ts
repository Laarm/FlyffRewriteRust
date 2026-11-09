// `WndDialogEvent` (`CWndDialogEvent`, `src:_Interface/WndField.cpp:21915-22200`, `__TRADESYS`, ref/22 §17 L, Plan/42 §19.4 L, 45 §11.2, §18.6, WP-UI-36
// shell; WP-CFEAT-26): `APP_DIALOG_EVENT` (304 x 432), "Conversation": the item-exchange list of an NPC. The upper text box shows the
// description (the arrow button `WIDC_BUTTON1` pages through several pages and hides on the last one), the list `WIDC_LISTBOX1` the entries of
// the script in order. OK with a chosen entry sends `ExchangeReq{mmi, index}` and disables itself until the answer; OK without a choice and Cancel
// close the window. The answer (`receiveResult`): success shows the first `RESULTMSG` text of the set, a full bag `TID_GAME_LACKSPACE`, a failed
// condition the second text, anything else "FAILED" - each in a message box - and OK works again. The window sizes itself to the list (up to 15
// entries): the list is 80 px wider and as high as its rows, the buttons sit below it, the text box is wider by 80.
//
// KEEP-DEVIATION: the row height is the list control's own pitch (the original measured the text "123" + 2); the red bar of `WIDC_CUSTOM2` over the
// chosen row is the list's own selection colour.

import {
  APP_DIALOG_EVENT,
  WIDC_BUTTON1,
  WIDC_CANCEL,
  WIDC_LISTBOX1,
  WIDC_OK,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { ListBox } from '../../controls/ListBox';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Rect, rectHeight } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  EXCHANGE_CONDITION_FAILED,
  EXCHANGE_FAILED,
  EXCHANGE_INVENTORY_FAILED,
  EXCHANGE_SUCCESS,
  type ExchangeRequest,
} from './ports';
import { ANSWER_TIMEOUT_MS, BUTTON_SHIFT, MAX_LIST_COUNT, WIDEN } from './geometry';

export class WndDialogEvent extends WndNeuz {
  readonly appId = APP_DIALOG_EVENT;
  readonly #req: ExchangeRequest;
  #list: ListBox | null = null;
  #text: TextCtrl | null = null;
  /** `m_nChoiceNum`. */
  #choice = -1;
  /** `m_nCurrentPage`. */
  #page = 0;
  /** The request is out and OK waits for the answer (`host.now()` of the send, `null` = none out). */
  #sentAt: number | null = null;

  constructor(req: ExchangeRequest) {
    super();
    this.#req = req;
    this.putRegInfo = false;
  }

  /** `SetMMI`'s menu. */
  get mmi(): number {
    return this.#req.mmi;
  }
  /** `m_nChoiceNum`, `-1` = none. */
  get choice(): number {
    return this.#choice;
  }
  get page(): number {
    return this.#page;
  }
  get list(): ListBox | null {
    return this.#list;
  }
  get descriptionText(): TextCtrl | null {
    return this.#text;
  }
  get okEnabled(): boolean {
    return this.getDlgItem(WIDC_OK)?.isWindowEnabled() ?? false;
  }
  get pageButtonVisible(): boolean {
    return this.getDlgItem(WIDC_BUTTON1)?.isVisible() ?? false;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const req = this.#req;
    if (req.entries.length === 0) {
      // `if( !m_nListCount ) Destroy()`
      this.destroy();
      return;
    }
    this.#list = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    for (const e of req.entries) this.#list?.addString(e);
    this.#describe();
    this.#size();
    this.moveParentCenter();
  }

  /** `SetDescription`: the first page and the paging button. */
  #describe(): void {
    const pages = this.#req.pages;
    if (pages.length === 0) return;
    this.#page = 0;
    this.#text?.setString(pages[0] ?? '');
    if (pages.length === 1) this.#hidePageButton();
  }

  #hidePageButton(): void {
    const b = this.getDlgItem<Button>(WIDC_BUTTON1);
    b?.setVisible(false);
    b?.enableWindow(false);
  }

  /** The size rules of `SetDescription`: the list fits its rows up to `MAX_LIST_COUNT`, everything is `WIDEN` px wider. */
  #size(): void {
    const list = this.#list;
    const count = this.#req.entries.length;
    const pitch = list?.pitch() ?? 15;
    const rectOf = (id: number): Rect | null => this.getWndCtrl(id)?.rect ?? null;
    const lr = rectOf(WIDC_LISTBOX1);
    const textRect = rectOf(WIDC_TEXT1);
    if (count <= MAX_LIST_COUNT && lr && textRect) {
      const bottom = lr.t + count * pitch + 10;
      const listHeight = bottom - lr.t;
      list?.setWndRect({ l: lr.l, t: lr.t, r: lr.r + WIDEN, b: bottom }, true);
      const w = this.getWindowRect(true);
      this.setWndRect({ ...w, b: w.t + listHeight + rectHeight(textRect) + 80 }, true);
      for (const id of [WIDC_OK, WIDC_CANCEL]) {
        const r = rectOf(id);
        if (!r) continue;
        const h = rectHeight(r);
        this.getDlgItem(id)?.setWndRect(
          { l: r.l + BUTTON_SHIFT, t: bottom + 8, r: r.r + BUTTON_SHIFT, b: bottom + 8 + h },
          true,
        );
      }
    }
    // always: the window, the paging button and the text box grow by `WIDEN`
    const w = this.getWindowRect(true);
    this.setWndRect({ ...w, r: w.r + WIDEN }, true);
    const b = rectOf(WIDC_BUTTON1);
    if (b) {
      this.getDlgItem(WIDC_BUTTON1)?.setWndRect(
        { l: b.l + WIDEN, t: b.t, r: b.r + WIDEN, b: b.b },
        true,
      );
    }
    if (textRect) {
      this.#text?.setWndRect(
        { l: textRect.l, t: textRect.t, r: textRect.r + WIDEN, b: textRect.b },
        true,
      );
    }
    this.requestBake();
  }

  // ---- the buttons --------------------------------------------------------------------------------------------------------------

  /** The arrow: the next description page; on the last page the arrow goes away. */
  nextPage(): void {
    const pages = this.#req.pages;
    if (this.#page >= pages.length - 1) return;
    this.#page += 1;
    this.#text?.setString(pages[this.#page] ?? '');
    if (this.#page === pages.length - 1) this.#hidePageButton();
  }

  /** OK: sends the chosen entry; without a choice the window closes. */
  ok(): void {
    if (this.#choice < 0) {
      this.destroy();
      return;
    }
    if (this.#sentAt !== null) return;
    this.#sentAt = this.env.host.now();
    this.getDlgItem(WIDC_OK)?.enableWindow(false);
    this.#req.send(this.#choice);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_LISTBOX1 && (msg === Wnm.SELCHANGE || msg === Wnm.CLICKED)) {
      const sel = this.#list?.getCurSel() ?? -1;
      if (sel !== -1) this.#choice = sel;
      return true;
    }
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.nextPage();
        return true;
      }
      if (id === WIDC_OK) {
        this.ok();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `ReceiveResult`: the message of the answer, then OK works again. */
  receiveResult(result: number): void {
    const req = this.#req;
    const pair = this.#choice >= 0 ? req.results(this.#choice) : null;
    switch (result) {
      case EXCHANGE_SUCCESS:
        if (pair) req.messageBox(pair[0]);
        break;
      case EXCHANGE_INVENTORY_FAILED:
        req.messageBox(req.lackSpace);
        break;
      case EXCHANGE_CONDITION_FAILED:
        if (pair) req.messageBox(pair[1]);
        break;
      case EXCHANGE_FAILED:
        req.messageBox('FAILED');
        break;
      default:
        break;
    }
    this.#sentAt = null;
    this.getDlgItem(WIDC_OK)?.enableWindow(true);
  }

  /** `Process`: a request without an answer for `ANSWER_TIMEOUT_MS` is given up (OK works again). */
  override process(now: number): void {
    super.process(now);
    if (this.#sentAt !== null && now - this.#sentAt >= ANSWER_TIMEOUT_MS) {
      this.#sentAt = null;
      this.getDlgItem(WIDC_OK)?.enableWindow(true);
    }
  }

  override onDestroy(): void {
    this.#req.closed?.();
    super.onDestroy();
  }
}
