// The lord windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-54, WP-UI-55): the lazy classes of `lazy.ts`, plus the lord skill APPLET (a plain class: the applet registry builds it
// synchronously). 10 layouts become `built`: 722, 718, 720, 724, 726, 728, 736, 738, 756, 758.
import type { WndCtor } from '../../catalog';
import { LORD_LAZY } from './lazy';
import { WndLordSkill } from './WndLordSkill';

export const LORD_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_LORD_STATE: () => LORD_LAZY.WndLordState.load(),
  APP_LORD_TENDER: () => LORD_LAZY.WndLordTender.load(),
  APP_LORD_PLEDGE: () => LORD_LAZY.WndLordPledge.load(),
  APP_LORD_VOTE: () => LORD_LAZY.WndLordVote.load(),
  APP_LORD_EVENT: () => LORD_LAZY.WndLordEvent.load(),
  APP_LORD_SKILL: () => Promise.resolve(WndLordSkill),
  APP_LORD_SKILL_CONFIRM: () => LORD_LAZY.WndLordSkillConfirm.load(),
  APP_LORD_CONFIRM: () => LORD_LAZY.WndLordConfirm.load(),
  APP_LORD_INFO: () => LORD_LAZY.WndLordInfo.load(),
  APP_LORD_INFO2: () => LORD_LAZY.WndLordRPInfo.load(),
};
