// `WndCoupleManager` (`CWndCoupleManager`, `src:_Interface/WndField.cpp:24065-24120`, ref/22 §17 K, Plan/42 §19.4 K, 45 §24.2; WP-UI-52): `APP_COUPLE_MAIN` (304 x 352, `WndTile00`, hotkey F,
// start menu `TID_GAME_COUPLE`), a tab control (`WIDC_TABCTRL1`) with the information page (`TID_GAME_COUPLE_INFO`) and the skill page (`TID_GAME_COUPLE_SKILL`). The pages are the layouts
// `APP_COUPLE_TAB_INFO` and `APP_COUPLE_TAB_SKILL` created as children of the tab control (`WBS_CHILD | WBS_NODRAWFRAME`); the window opens centred.

import { APP_COUPLE_MAIN, WIDC_TABCTRL1 } from '../../../gen/defs/resdata';
import { TID_GAME_COUPLE_INFO, TID_GAME_COUPLE_SKILL } from '../../../gen/defs/text';
import type { TabCtrl } from '../../controls/TabCtrl';
import { WBS_CHILD, WBS_NODRAWFRAME, WBS_NOFRAME } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CoupleInfoPage } from './CoupleInfoPage';
import { CoupleSkillPage } from './CoupleSkillPage';
import type { CouplePorts } from './ports';

export interface CoupleManagerDeps {
  readonly ports: CouplePorts;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

export class WndCoupleManager extends WndNeuz {
  readonly appId = APP_COUPLE_MAIN;
  readonly #deps: CoupleManagerDeps;
  #tab: TabCtrl | null = null;
  #info: CoupleInfoPage | null = null;
  #skill: CoupleSkillPage | null = null;

  constructor(deps: CoupleManagerDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get infoPage(): CoupleInfoPage | null {
    return this.#info;
  }
  get skillPage(): CoupleSkillPage | null {
    return this.#skill;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      this.#tab = tab;
      const { ports, tooltip, atlases, getText } = this.#deps;
      const style = WBS_CHILD | WBS_NODRAWFRAME | WBS_NOFRAME;
      const info = new CoupleInfoPage({ view: ports.info, actions: ports.actions, atlases });
      info.initialize(tab, style);
      const skill = new CoupleSkillPage({ view: ports.skill, actions: ports.actions, tooltip });
      skill.initialize(tab, style);
      this.#info = info;
      this.#skill = skill;
      tab.insertItem(0, { text: getText(TID_GAME_COUPLE_INFO), page: info });
      tab.insertItem(1, { text: getText(TID_GAME_COUPLE_SKILL), page: skill });
      tab.setCurSel(0);
    }
    this.moveParentCenter();
  }
}
