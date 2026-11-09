// The couple and honor windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-52): the applet and the three pages are plain classes (the applet registry and the
// character window build them synchronously). The couple message window is a box on `APP_MESSAGEBOX` and has no catalog row of its own (`lazy.ts`).
import type { WndCtor } from '../../catalog';
import { CoupleInfoPage } from './CoupleInfoPage';
import { CoupleSkillPage } from './CoupleSkillPage';
import { HonorPage } from './HonorPage';
import { WndCoupleManager } from './WndCoupleManager';

export const COUPLE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_COUPLE_MAIN: () => Promise.resolve(WndCoupleManager),
  APP_COUPLE_TAB_INFO: () => Promise.resolve(CoupleInfoPage),
  APP_COUPLE_TAB_SKILL: () => Promise.resolve(CoupleSkillPage),
  APP_HONOR: () => Promise.resolve(HonorPage),
};
