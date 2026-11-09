// `WndMotion` (`CWndMotion`, `src:_Interface/WndMotion.cpp:1-95`, ref/22 §17 D, Plan/42 §19.4 D, 45 §19.4; WP-UI-21):
// `APP_MOTION` (304 x 320), a tab control with the motions (`TID_GAME_TOOLTIP_MOTION`) and the chat emoticons
// (`TID_GAME_TOOLTIP_EMOTICON`). Both pages are icon grids that execute on a click and are drag sources for the taskbar.

import { APP_MOTION, WIDC_TABCTRL1 } from '../../../gen/defs/resdata';
import { TID_GAME_TOOLTIP_EMOTICON, TID_GAME_TOOLTIP_MOTION } from '../../../gen/defs/text';
import type { TabCtrl } from '../../controls/TabCtrl';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import { EmoticonPage, MotionPage } from './MotionPages';
import type { MotionActions, MotionView, ViewSource } from './ports';

export interface MotionDeps {
  readonly view: ViewSource<MotionView>;
  readonly actions: MotionActions;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

export class WndMotion extends WndNeuz {
  readonly appId = APP_MOTION;
  readonly #deps: MotionDeps;
  #tab: TabCtrl | null = null;
  #motions: MotionPage | null = null;
  #emoticons: EmoticonPage | null = null;

  constructor(deps: MotionDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get motionPage(): MotionPage | null {
    return this.#motions;
  }
  get emoticonPage(): EmoticonPage | null {
    return this.#emoticons;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      this.#tab = tab;
      const motions = new MotionPage(this.#deps);
      motions.attach(tab, 100000);
      const emoticons = new EmoticonPage(this.#deps);
      emoticons.attach(tab, 1000001);
      this.#motions = motions;
      this.#emoticons = emoticons;
      tab.insertItem(0, { text: this.#deps.getText(TID_GAME_TOOLTIP_MOTION), page: motions });
      tab.insertItem(1, { text: this.#deps.getText(TID_GAME_TOOLTIP_EMOTICON), page: emoticons });
      tab.setCurSel(0);
    }
    // the window opens in the top-left corner of the layout rect
    const layout = this.env.wm.layoutRect;
    this.move({ x: layout.l, y: layout.t });
  }
}
