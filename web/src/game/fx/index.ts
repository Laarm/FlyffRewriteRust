// The effect triggers of the client (Plan/41 section 17.3; WP-REND-18/20): `fxModule` is registered in `RUNTIME_MODULES` of
// `app/features.ts`. It owns the `Phase.Fx` handlers of the messages that create effects and the sim-event subscription of the
// generic hit effects; the renderer side is `render/sfx`.

import type { HandlerModule } from '../../app/features';
import { registerBossHandlers } from './boss';
import { registerFxHandlers } from './handlers';
import { registerM7Handlers } from './m7';
import { fxRuntime } from './runtime';

export const fxModule: HandlerModule = {
  id: 'fx',
  register(ctx) {
    const rt = fxRuntime(ctx);
    registerFxHandlers(ctx, rt);
    // ---- render-audio-m7 (WP-REND-27): the boss triggers, the event balloon and the room papering
    const boss = registerBossHandlers(ctx, rt);
    const m7 = registerM7Handlers(ctx, rt);
    if (!ctx.probe.sections().includes('fx')) {
      ctx.probe.extend('fx', {
        stats: () => ({ ...rt.stats }),
        levels: () => Object.fromEntries(rt.levels),
        boss,
        m7,
      });
    }
  },
};

export { fxRuntime, peekFxRuntime, type FxHooks, type FxRuntime, type FxStats } from './runtime';
export { hitSparks, validEffect, type HitSpark } from './hitSpark';
export { inJobLine, levelUpEffect } from './levelUp';
