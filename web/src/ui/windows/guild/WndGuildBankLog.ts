// `WndGuildBankLog` (`CWndGuildBankLog`, `src:_Interface/WndGuildBank.cpp:330-700`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-47, WP-CFEAT-38): `APP_GUILD_BANK_LOG` (496 x 304), the
// log of the guild bank. One tab control (`WIDC_TABCTRL1`) with four pages made by code - items added, items removed, penya received, penya invested (`TID_GAME_GUILD_BANK_*LOG`) -
// each a list of formatted lines (`BankLogList`, 20 px apart from (4, 3), a docked scroll bar). The window asks for the list of the selected tab (`request( kind )`, kinds 1..4): the
// first one when it opens and again with every tab change; the answer is the feature's (it keeps the lines whether or not the window is open: FIX of the original, which dropped an
// answer when the window was closed). `WIDC_OK` closes. The window opens beside the guild bank: to its right, or to its left when there is no room.

import {
  APP_GUILD_BANK,
  APP_GUILD_BANK_LOG,
  WIDC_OK,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILD_BANK_ADDITEMLOG,
  TID_GAME_GUILD_BANK_INVESTPENYALOG,
  TID_GAME_GUILD_BANK_RECEIVEPENYALOG,
  TID_GAME_GUILD_BANK_REMOVEITEMLOG,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { TabCtrl } from '../../controls/TabCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CodePage } from '../char/CodePage';
import { BankLogList } from './BankLogList';
import type { BankLogKind, BankLogRequest, BankLogView } from './ports';

const ID_PAGE = 11;
const ID_LIST = 0x7f51;
const KINDS: readonly BankLogKind[] = [1, 2, 3, 4];

/** One tab page: a `BankLogList` over its whole client area. */
class LogPage extends CodePage {
  list: BankLogList | null = null;

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.list = this.addControl(new BankLogList(), ID_LIST, this.getClientRect(false));
  }

  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    this.list?.setWndRect({ l: 0, t: 0, r: cx, b: cy });
  }
}

export interface BankLogDeps {
  readonly getText: (tid: number) => string;
}

export class WndGuildBankLog extends WndNeuz {
  readonly appId = APP_GUILD_BANK_LOG;
  readonly #request: BankLogRequest;
  readonly #deps: BankLogDeps;
  readonly #pages: LogPage[] = [];
  #tab: TabCtrl | null = null;
  #applied: BankLogView | null = null;
  #off: (() => void)[] = [];

  constructor(request: BankLogRequest, deps: BankLogDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  /** The list of a kind (1..4). */
  list(kind: BankLogKind): BankLogList | null {
    return this.#pages[kind - 1]?.list ?? null;
  }
  /** The kind of the selected tab (1..4). */
  get kind(): BankLogKind {
    return KINDS[this.#tab?.getCurSel() ?? 0] ?? 1;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    this.#tab = tab;
    if (tab) {
      const texts = [
        TID_GAME_GUILD_BANK_ADDITEMLOG,
        TID_GAME_GUILD_BANK_REMOVEITEMLOG,
        TID_GAME_GUILD_BANK_RECEIVEPENYALOG,
        TID_GAME_GUILD_BANK_INVESTPENYALOG,
      ];
      texts.forEach((tid, i) => {
        const page = new LogPage();
        page.attach(tab, ID_PAGE + i);
        this.#pages.push(page);
        tab.insertItem(i, { text: this.#deps.getText(tid), page });
      });
    }
    this.#apply(this.#request.view.get());
    this.#off = [
      this.#request.view.subscribe(() => {
        this.#apply(this.#request.view.get());
      }),
    ];
    this.#place();
    // `m_wndAddItemLog.UpdateLogList()`: the first list is asked for at once
    this.#request.request(1);
  }

  /** The beam of the bank: to its right (`ptBank.BottomRight + (10, -height)`), to its left when the root has no room. */
  #place(): void {
    const wm = this.env.wm;
    const bank = wm.get(APP_GUILD_BANK);
    const root = wm.layoutRect;
    const w = rectWidth(this.rectWindow);
    const h = rectHeight(this.rectWindow);
    if (!bank) {
      this.move({ x: root.r - w, y: root.b - h });
      return;
    }
    const b = bank.rectWindow;
    if (root.r - b.r < w) this.move({ x: b.l - (10 + w), y: b.t });
    else this.move({ x: b.r + 10, y: b.t });
  }

  #apply(v: BankLogView): void {
    this.#applied = v;
    KINDS.forEach((_kind, i) => {
      this.#pages[i]?.list?.setLines(v.lines[i] ?? []);
    });
  }

  override process(now: number): void {
    const v = this.#request.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_TABCTRL1 && msg === Wnm.SELCHANGE) {
      this.#request.request(this.kind);
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    for (const off of this.#off) off();
    this.#off = [];
    this.#request.closed?.();
    super.onDestroy();
  }
}
