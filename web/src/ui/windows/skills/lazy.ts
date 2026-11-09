// The skill helper windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The skill tree itself is an
// applet (`APP_SKILL3`) built synchronously by the applet registry, so its class is a plain import.
import { LazyWnd } from '../../wnd/lazy';
import type * as HeroSkillUp from './WndHeroSkillUp';
import type * as ReSkillControl from './WndReSkillControl';

export const SKILLS_LAZY = {
  WndReSkillControl: new LazyWnd<typeof ReSkillControl.WndReSkillControl>(
    'WndReSkillControl',
    async () => (await import('./WndReSkillControl')).WndReSkillControl,
  ),
  WndHeroSkillUp: new LazyWnd<typeof HeroSkillUp.WndHeroSkillUp>(
    'WndHeroSkillUp',
    async () => (await import('./WndHeroSkillUp')).WndHeroSkillUp,
  ),
} as const;
