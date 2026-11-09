// `HonorPage` (`CWndHonor`, `src:_Interface/WndField.cpp:4452-4590`, ref/22 §17 O, Plan/42 §19.4 O, 45 §24.3; WP-UI-52): the honor tab of the character window (`APP_HONOR`, 192 x 432, the second
// page of `APP_CHARACTER3`). The list box holds "No Title" followed by the earned titles in list order, only when at least one is earned; with none the text "No fame obtained." is
// drawn black at ( list.left + 5, list.top + 8 ). The OK button is disabled at every refresh; a click on a row enables it and remembers the title (row 0: -1, row n: the n-th earned title);
// OK disables it again and sends the change (KEEP: nothing refreshes after the answer, so the button stays disabled until the next click on a row).
//
// The title above a name is NOT drawn here: `visual.title` is set by the feature and the nameplates of 41 print `[title] name`.

import { APP_HONOR, WIDC_BUTTON1, WIDC_LISTBOX1 } from '../../../gen/defs/resdata';
import { TID_GAME_NO_TITLE, TID_GAME_NOT_SELECTED_TITLE } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { ListBox } from '../../controls/ListBox';
import { TextLayer } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  EMPTY_HONOR_VIEW,
  type HonorActions,
  type HonorPorts,
  type HonorView,
  type ViewSource,
} from './ports';

/** `m_nSelectColor = D3DCOLOR_ARGB( 255, 255, 0, 0 )`. */
export const HONOR_SELECT_COLOR = 0xffff0000;
/** `dwNormal`: the colour of the empty-list text. */
export const HONOR_EMPTY_COLOR = 0xff000000;
const ID_TEXTS = 0x7e81;

export interface HonorDeps {
  readonly view: ViewSource<HonorView>;
  readonly actions: HonorActions;
  readonly getText: (tid: number) => string;
}

/** The ports of the page as {@link HonorDeps}. */
export function honorDeps(ports: HonorPorts, getText: (tid: number) => string): HonorDeps {
  return { view: ports.view, actions: ports.actions, getText };
}

export class HonorPage extends WndNeuz {
  readonly appId = APP_HONOR;
  readonly #deps: HonorDeps;
  #texts: TextLayer | null = null;
  #applied: HonorView = EMPTY_HONOR_VIEW;
  /** `m_nSelectedId`: the title to send, `null` until a row was clicked. */
  #pending: number | null = null;
  #unsubscribe: (() => void) | null = null;

  constructor(deps: HonorDeps) {
    super();
    this.#deps = deps;
  }

  /** `WIDC_LISTBOX1`. */
  get list(): ListBox | null {
    return this.getDlgItem<ListBox>(WIDC_LISTBOX1);
  }
  /** `WIDC_BUTTON1` (OK). */
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get textLayer(): TextLayer | null {
    return this.#texts;
  }
  /** The title id the OK button would send, `null` when no row was clicked since the last refresh. */
  get pending(): number | null {
    return this.#pending;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.list?.setSelectColor(HONOR_SELECT_COLOR);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, this.getClientRect(false));
    this.refresh();
    this.#unsubscribe = this.#deps.view.subscribe(() => {
      this.#syncView();
    });
    // the original never asked (the join carries the counters); the plan asks once when the window opens, which is harmless
    this.#deps.actions.opened();
  }

  override process(now: number): void {
    this.#syncView();
    super.process(now);
  }

  #syncView(): void {
    if (this.#deps.view.get() !== this.#applied) this.refresh();
  }

  /** `RefreshList`: the list box starts over, OK is disabled, "No Title" and the earned titles are added when there are any. */
  refresh(): void {
    const v = this.#deps.view.get();
    this.#applied = v;
    this.#pending = null;
    const list = this.list;
    list?.resetContent();
    this.okButton?.enableWindow(false);
    if (list && v.titles.length > 0) {
      list.addString(this.#deps.getText(TID_GAME_NOT_SELECTED_TITLE));
      for (const t of v.titles) list.addString(t.title);
    }
    this.#paintEmpty(v);
  }

  /** `OnDraw`: with no title earned, "No fame obtained." at ( left + 5, top + 8 ) of the list. */
  #paintEmpty(v: HonorView): void {
    const at = this.getWndCtrl(WIDC_LISTBOX1)?.rect;
    this.#texts?.setItems(
      v.titles.length === 0 && at
        ? [
            {
              x: at.l + 5,
              y: at.t + 8,
              text: this.#deps.getText(TID_GAME_NO_TITLE),
              color: HONOR_EMPTY_COLOR,
            },
          ]
        : [],
    );
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.SELCHANGE && id === WIDC_LISTBOX1) {
      const sel = this.list?.getCurSel() ?? -1;
      // FIX: a click on the empty area with no row selected (the original indexed `m_vecTitle[ -2 ]`)
      if (sel < 0) return true;
      const entry = sel === 0 ? null : this.#applied.titles[sel - 1];
      if (sel > 0 && !entry) return true;
      this.#pending = entry ? entry.id : -1;
      this.okButton?.enableWindow(true);
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      const pending = this.#pending;
      // a disabled OK sends nothing (it stays disabled until the next click on a row)
      if (pending === null || this.okButton?.isWindowEnabled() === false) return true;
      this.okButton?.enableWindow(false);
      this.#deps.actions.change(pending);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    super.onDestroy();
  }
}
