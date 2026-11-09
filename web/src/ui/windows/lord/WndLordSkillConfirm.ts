// `WndLordSkillConfirm` (`CWndLordSkillConfirm`, `src:_Interface/WndLord.cpp:973-1038`, ref/22 §17 O, Plan/42 §19.4 O; WP-UI-54): `APP_LORD_SKILL_CONFIRM` (736, 272 x 208), the name prompt of the two
// lord skills that need a target by name: SUMMON asks "Which character do you want to summon?", TELEPORT "... to teleport to?" (`TID_GAME_LORD_SKILL_CONFIRM1` / `2`); the edit carries the tooltip
// `TID_TIP_LORD_SKILL_CONFIRM1` / `2`. `BUTTON1` sends `(type, text)` and closes; an empty name is sent as it is (KEEP: the server answers `TID_GAME_LORD_SKILL_USE_E004`).

import {
  APP_LORD_SKILL_CONFIRM,
  WIDC_BUTTON1,
  WIDC_EDIT1,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_LORD_SKILL_CONFIRM1,
  TID_GAME_LORD_SKILL_CONFIRM2,
  TID_TIP_LORD_SKILL_CONFIRM1,
  TID_TIP_LORD_SKILL_CONFIRM2,
} from '../../../gen/defs/text';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { LI_SUMMON, LI_TELEPORT, type LordWindowDeps } from './ports';

export class WndLordSkillConfirm extends WndNeuz {
  readonly appId = APP_LORD_SKILL_CONFIRM;
  readonly #type: number;
  readonly #deps: LordWindowDeps;

  /** `type` is the lord skill: `LI_SUMMON` (5) or `LI_TELEPORT` (6). */
  constructor(type: number, deps: LordWindowDeps) {
    super();
    this.#type = type;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get skill(): number {
    return this.#type;
  }
  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const host = this.#deps.host;
    if (this.#type === LI_SUMMON) {
      this.textCtrl?.setString(host.text(TID_GAME_LORD_SKILL_CONFIRM1));
      this.edit?.setToolTip(host.text(TID_TIP_LORD_SKILL_CONFIRM1));
    } else if (this.#type === LI_TELEPORT) {
      this.textCtrl?.setString(host.text(TID_GAME_LORD_SKILL_CONFIRM2));
      this.edit?.setToolTip(host.text(TID_TIP_LORD_SKILL_CONFIRM2));
    }
    this.moveParentCenter();
  }

  /** `BUTTON1`: sends the name and closes (`m_nType != 0` is the original's only check). */
  confirm(): boolean {
    if (this.#type === 0) return false;
    this.#deps.ports.actions.useSkill(this.#type, this.edit?.getString() ?? '');
    this.destroy();
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.confirm();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
