// The skill-tree windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-38): the tree is an applet (a plain class), the helper windows are
// lazy classes (`lazy.ts`).
import type { WndCtor } from '../../catalog';
import { SKILLS_LAZY } from './lazy';
import { WndSkillTree } from './WndSkillTree';

export const SKILL_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_SKILL3: () => Promise.resolve(WndSkillTree),
  APP_RESKILL_CONTROL1: () => SKILLS_LAZY.WndReSkillControl.load(),
  APP_HERO_SKILLUP: () => SKILLS_LAZY.WndHeroSkillUp.load(),
};
