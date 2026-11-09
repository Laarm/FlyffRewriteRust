// The boss effect triggers of the client (Plan/41 section 17.1 "Bosses", 17.3; WP-REND-27): the sim events and messages that drive `render/sfx/bossFx.ts`, the camera quake of `CommonPlace`, and
// the Mushmoot's growth (`SetScale`). The visual effects themselves live in the renderer (`RenderFx.bossMeleeHit` / `bossCharge`); this module only decides WHO (the boss models) and passes the
// numbers on.
//
//   AttrHit (sim)       Melee hits of the boss models -> `fx.bossMeleeHit(attacker, target, state, hitCount)` (`words = [kind, state, target, hitCount]`; the client sim counts the hits of a boss
//                       swing, `flyff-sim` `process_attack_combo`). `MI_LOADCLOCKWORK` only with `features.better_clockwork_alpha`; every other mover (also `MI_EVENT04`) is ignored
//   BossChargeFx (sim)  -> `fx.bossCharge(id, state)`
//   CommonPlace QUAKE   `CWorld::m_pCamera->SetQuake( 1400, 1.0f )` for ANY object id (`OnCommonPlace` asks `GetCtrl`, a mover id works) while `features.no_quake` is false
//   SetScale            `OnSetScale` + `ProcessScaleSlerp`: the growth of a RANK_MATERIAL / SUPER / MIDBOSS mover (the Mushmoot after ATK4) toward `model scale x factor`: per 60 Hz step
//                       `scale = dest + s * (scale - dest)`, then `s -= 0.001` while `s > 0` (1 000 steps = 16.7 s, KEEP). The scale is written through the entity's `visual.scale`, which the
//                       sim's mover params copy (`buildMoverParams`), and `params.mark`

import type { ClientContext } from '../../app/context';
import { RANK_MATERIAL, RANK_MIDBOSS, RANK_SUPER } from '../../gen/defs/attribute';
import { OT_MOVER } from '../../gen/defs/define';
import { Phase } from '../../net/registry';
import type { SimEventRecord } from '../simEvents';
import { peekWorldRuntime } from '../world/runtime';
import { bossKindOf } from './bossIds';
import type { FxRuntime } from './runtime';

const OWNER = 'fx.boss';
/** `HitKind::Melee` as the first `AttrHit` word. */
const HIT_KIND_MELEE = 0;
/** `COMMONPLACE_QUAKE`. */
export const COMMONPLACE_QUAKE = 2;
/** `SetQuake( 1400, 1.0f )` of `OnCommonPlace`. */
export const COMMONPLACE_QUAKE_TICKS = 1400;
export const COMMONPLACE_QUAKE_SIZE = 1.0;
/** `m_fDestScaleSlerp -= 0.001f`. */
export const SCALE_SLERP_STEP = 0.001;

/** The growth of one mover (`m_fDestScale`, `m_fDestScaleSlerp`). */
export interface ScaleSlerp {
  dest: number;
  s: number;
}

/** One step of `ProcessScaleSlerp`: `scale = dest + s * (scale - dest)`, then `s -= 0.001`; `s <= 0` means done. */
export function slerpStep(scale: number, g: ScaleSlerp): number {
  if (!(g.s > 0)) return scale;
  const next = Math.fround(g.dest + g.s * (scale - g.dest));
  g.s = Math.fround(g.s - SCALE_SLERP_STEP);
  return next;
}

export function registerBossHandlers(
  ctx: ClientContext,
  rt: FxRuntime,
): () => Record<string, unknown> {
  const { registry } = ctx.net;
  const better = (): boolean => ctx.features.features.better_clockwork_alpha;
  const growing = new Map<number, ScaleSlerp>();

  const isBoss = (id: number): boolean => {
    const rec = ctx.entities.get(id);
    return rec !== undefined && bossKindOf(rec.propId, better()) !== undefined;
  };

  // ---- the hit handlers and the charge (sim events) -------------------------------------------------------------------------
  ctx.sim.events.on(['AttrHit'], (e: SimEventRecord) => {
    if (e.words[0] !== HIT_KIND_MELEE || !isBoss(e.objId)) return;
    const fx = rt.fx();
    if (!fx?.bossMeleeHit) return;
    rt.stats.bossHits += 1;
    fx.bossMeleeHit(e.objId, e.words[2], e.words[1], e.words[3]);
  });
  ctx.sim.events.on(['BossChargeFx'], (e: SimEventRecord) => {
    if (!isBoss(e.objId)) return;
    const fx = rt.fx();
    if (!fx?.bossCharge) return;
    rt.stats.bossCharges += 1;
    fx.bossCharge(e.objId, e.words[0]);
  });

  // ---- CommonPlace QUAKE ---------------------------------------------------------------------------------------------------------
  registry.on('commonPlace', Phase.Fx, OWNER, (_objId, m) => {
    if (m.kind !== COMMONPLACE_QUAKE) return;
    if (ctx.features.features.no_quake) return;
    rt.stats.quakes += 1;
    ctx.input.quake?.(COMMONPLACE_QUAKE_TICKS, COMMONPLACE_QUAKE_SIZE);
  });

  // ---- SetScale ------------------------------------------------------------------------------------------------------------------
  registry.on('setScale', Phase.Sim, OWNER, (objId, m) => {
    const rec = ctx.entities.get(objId);
    if (!rec?.visual) return;
    const rank = ctx.data?.mover(rec.propId)?.class;
    // `ProcessScaleSlerp`: anything but a material / super / mid-boss mover is the original's error branch (the growth is dropped)
    if (rank !== RANK_MATERIAL && rank !== RANK_SUPER && rank !== RANK_MIDBOSS) return;
    const model = ctx.data?.model(OT_MOVER, rec.propId)?.scale ?? 1;
    growing.set(objId, { dest: Math.fround(model * m.factor), s: 1 });
    rt.stats.scaleSet += 1;
  });
  ctx.tickSystems.register('fx.bossScale', (due) => {
    if (growing.size === 0) return;
    const world = peekWorldRuntime(ctx);
    for (const [id, g] of growing) {
      const rec = ctx.entities.get(id);
      const v = rec?.visual;
      if (!rec || !v) {
        growing.delete(id);
        continue;
      }
      for (let i = 0; i < due && g.s > 0; i++) v.scale = slerpStep(v.scale, g);
      rt.stats.scaleSteps += due;
      world?.params.mark(id);
      if (!(g.s > 0)) growing.delete(id);
    }
  });
  registry.on('delObj', Phase.Fx, OWNER, (objId) => {
    growing.delete(objId);
  });

  /** `__flyffDebug.fx.boss()`. */
  return () => ({
    hits: rt.stats.bossHits,
    charges: rt.stats.bossCharges,
    quakes: rt.stats.quakes,
    scaleSet: rt.stats.scaleSet,
    scaleSteps: rt.stats.scaleSteps,
    growing: [...growing].map(([id, g]) => ({ id, dest: g.dest, s: g.s })),
  });
}
