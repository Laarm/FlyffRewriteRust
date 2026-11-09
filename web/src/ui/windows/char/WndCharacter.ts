// `WndCharacter` (`CWndCharacter`, `src:_Interface/WndField.cpp:4593-4745`, ref/22 §17 D, Plan/42 §19.4 D; WP-UI-21):
// `APP_CHARACTER3` (208 x 512), a tab control with the information page (`TID_GAME_TITLE_CHAR_INFO`, built by code, see
// `CharInfoPage`) and the honor page (`TID_GAME_TITLE_HONOR`, the layout `APP_HONOR` as `HonorPage` of `windows/couple`, WP-UI-52).
// The selected tab is stored with the window rect (tab 2 and beyond fall back to 0).

import { APP_CHARACTER3, WIDC_TABCTRL1 } from '../../../gen/defs/resdata';
import { TID_GAME_TITLE_CHAR_INFO, TID_GAME_TITLE_HONOR } from '../../../gen/defs/text';
import type { TabCtrl } from '../../controls/TabCtrl';
import { WBS_CHILD, WBS_NODRAWFRAME, WBS_NOFRAME } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import { HonorPage, type HonorDeps } from '../couple/HonorPage';
import { CharInfoPage } from './CharInfoPage';
import type { CharacterActions, CharacterView, StateConfirmRequest, ViewSource } from './ports';

/** `m_wndCharInfo.Create(.., lpTapCtrl, 100000)`. */
export const CHAR_INFO_PAGE_ID = 100000;

export interface CharacterDeps {
  readonly view: ViewSource<CharacterView>;
  readonly actions: CharacterActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  readonly openStateConfirm: (request: StateConfirmRequest) => void;
  /** The ports of the honor tab (`HonorPage`). */
  readonly honor: HonorDeps;
}

export class WndCharacter extends WndNeuz {
  readonly appId = APP_CHARACTER3;
  readonly #deps: CharacterDeps;
  #tab: TabCtrl | null = null;
  #info: CharInfoPage | null = null;
  #honor: HonorPage | null = null;

  constructor(deps: CharacterDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get infoPage(): CharInfoPage | null {
    return this.#info;
  }
  /** The honor tab: the layout `APP_HONOR` as `HonorPage` (WP-UI-52). */
  get honorPage(): HonorPage | null {
    return this.#honor;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (!tab) return;
    this.#tab = tab;
    const info = new CharInfoPage(this.#deps);
    info.attach(tab, CHAR_INFO_PAGE_ID, WBS_CHILD | WBS_NOFRAME);
    this.#info = info;
    const honor = new HonorPage(this.#deps.honor);
    honor.initialize(tab, WBS_CHILD | WBS_NODRAWFRAME | WBS_NOFRAME);
    this.#honor = honor;
    tab.insertItem(0, { text: this.#deps.getText(TID_GAME_TITLE_CHAR_INFO), page: info });
    tab.insertItem(1, { text: this.#deps.getText(TID_GAME_TITLE_HONOR), page: honor });
    tab.setCurSel(0);
  }

  /** The selected tab goes with the window rect (`SerializeRegInfo`). */
  override serializeRegInfo(): unknown {
    return this.#tab?.getCurSel() ?? 0;
  }
  override restoreRegInfo(v: unknown): void {
    const n = typeof v === 'number' ? v : 0;
    this.#tab?.setCurSel(n > 1 ? 0 : n);
  }
}
