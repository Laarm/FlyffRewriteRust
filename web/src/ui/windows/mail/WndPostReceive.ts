// `WndPostReceive` (`CWndPostReceive`, `src:_Interface/WndField.cpp:15531-15931`, ref/22 §17 I, Plan/42 §19.4 I; WP-UI-27): the inbox tab of the post window,
// `APP_POST_RECEIVE` (300 x 320, a layout with no controls). The rows are the `MailList` of this folder over the whole page: a press selects a mail, a
// double click opens it (`APP_POST_READ`), the pointer shows the date of arrival, the wheel and the bar scroll. The box itself arrives from the server;
// the page only shows what the view has.

import { APP_POST_RECEIVE, APP_POST_READ } from '../../../gen/defs/resdata';
import { rect } from '../../layout/geometry';
import type { TooltipService } from '../../tooltip/TooltipService';
import { LayoutPage } from './LayoutPage';
import { MailList } from './MailList';
import { WIDC_MAIL_LIST } from './names';
import type { MailRow, MailView, PostActions, ViewSource } from './ports';

export interface PostReceiveDeps {
  readonly view: ViewSource<MailView>;
  readonly actions: Pick<PostActions, 'openMail'>;
  readonly tooltip: TooltipService;
}

export class WndPostReceive extends LayoutPage {
  readonly appId = APP_POST_RECEIVE;
  readonly #deps: PostReceiveDeps;
  #list: MailList | null = null;
  #applied: MailView | null = null;
  #off: (() => void) | null = null;

  constructor(deps: PostReceiveDeps) {
    super();
    this.#deps = deps;
  }

  /** The inbox list. */
  get list(): MailList | null {
    return this.#list;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const list = new MailList({
      open: (row) => {
        this.#deps.actions.openMail(row.id);
      },
      hover: (row, r) => {
        if (row && r) this.#deps.tooltip.put(this, row.receivedText, r, 0);
        else this.#deps.tooltip.cancel();
      },
    });
    this.#list = this.addControl(list, WIDC_MAIL_LIST, this.getClientRect(false));
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    this.#list?.setWndRect(rect(0, 0, cx, cy));
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #apply(v: MailView): void {
    this.#applied = v;
    this.#list?.setNewFontColor(v.newFontColor);
    this.#list?.setRows(v.rows);
  }

  /** The reader window of the selected mail is open (the original kept `m_wndPostRead`). */
  get readerOpen(): boolean {
    return this.env.wm.isOpen(APP_POST_READ);
  }

  /** The mail the list has selected. */
  get selected(): MailRow | null {
    const id = this.#list?.selectedMailId ?? -1;
    return this.#deps.view.get().rows.find((r) => r.id === id) ?? null;
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#deps.tooltip.cancel();
    super.onDestroy();
  }
}
