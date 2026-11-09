// The effect runtime of one `ClientContext` (Plan/41 section 17.3; WP-REND-18/20): the hooks the options plug in, the counters of the
// probe (`__flyffDebug.fx`) and the levels the level-up handler remembers.

import type { ClientContext } from '../../app/context';
import type { RenderFx } from '../../render/fx';

export interface FxHooks {
  /** The option "hide pets" (`g_Option.m_OffPets`): the spawn effect of a pet is suppressed. */
  hidePets?(): boolean;
}

export interface FxStats {
  createSfxObj: number;
  removeSfxObj: number;
  levelUps: number;
  spawnEffects: number;
  hitSparks: number;
  /** The dust effects of the skid and drift events. */
  dust: number;
  /** Requests dropped because the renderer did not exist yet. */
  noRenderer: number;
  // ---- render-audio-m7 (WP-REND-27)
  /** Boss melee hits and charges handed to the renderer. */
  bossHits: number;
  bossCharges: number;
  /** `CommonPlace` quakes that reached the camera, `SetScale` growths accepted and their steps. */
  quakes: number;
  scaleSet: number;
  scaleSteps: number;
  /** Event balloons handed to the renderer and papering changes forwarded (`m7.ts`). */
  balloons: number;
  papering: number;
}

export interface FxRuntime {
  readonly hooks: FxHooks;
  readonly stats: FxStats;
  /** The last known level of every player in view (spawn and `SetLevel`). */
  readonly levels: Map<number, number>;
  /** The effect API at this moment (`undefined` until the renderer exists). */
  fx(): RenderFx | undefined;
}

const runtimes = new WeakMap<ClientContext, FxRuntime>();

export function peekFxRuntime(ctx: ClientContext): FxRuntime | undefined {
  return runtimes.get(ctx);
}

export function fxRuntime(ctx: ClientContext): FxRuntime {
  const known = runtimes.get(ctx);
  if (known) return known;
  const rt: FxRuntime = {
    hooks: {},
    stats: {
      createSfxObj: 0,
      removeSfxObj: 0,
      levelUps: 0,
      spawnEffects: 0,
      hitSparks: 0,
      dust: 0,
      noRenderer: 0,
      bossHits: 0,
      bossCharges: 0,
      quakes: 0,
      scaleSet: 0,
      scaleSteps: 0,
      balloons: 0,
      papering: 0,
    },
    levels: new Map(),
    fx: () => ctx.render?.fx,
  };
  runtimes.set(ctx, rt);
  return rt;
}
