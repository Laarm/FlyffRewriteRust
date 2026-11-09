// The client combat runtime (Plan/17 section 11; WP-COMBAT-10): the handlers of the M3 combat messages, the `Damage`
// presentation decision tree, the projectile visuals and the hooks other documents plug in. `combatModule` is registered in
// `RUNTIME_MODULES` of `app/features.ts`.

import type { HandlerModule } from '../../app/features';
import { EntityKind, type EntityRecord } from '../entities';
import { registerBuffFx } from './buffs';
import { registerCombatHandlers } from './handlers';
import { combatRuntime } from './runtime';
import { registerSkillHandlers } from './skills';

export const combatModule: HandlerModule = {
  id: 'combat',
  register(ctx) {
    const rt = combatRuntime(ctx);
    registerCombatHandlers(ctx, rt);
    // the skill messages and the buff loop effects (M5, WP-SKILL-12)
    const skills = registerSkillHandlers(ctx);
    const buffFx = registerBuffFx(ctx);
    if (!ctx.probe.sections().includes('combat')) {
      const at: [number, number, number] = [0, 0, 0];
      /** A mover as the e2e specs look at it: where it stands, how much HP it has and whether it is a monster. */
      const describe = (rec: EntityRecord): ProbeMover => {
        const view = ctx.sim.view;
        const known = view?.valid === true && view.posOf(rec.objId, at);
        return {
          objId: rec.objId,
          name: rec.visual?.name ?? '',
          monster: rec.visual?.kind === 'monster',
          hp: rec.hp,
          maxHp: rec.maxHp ?? 0,
          peaceful: rec.visual?.peaceful ?? false,
          pos: known ? [at[0], at[1], at[2]] : null,
        };
      };
      ctx.probe.extend('combat', {
        /** The monsters (and NPCs) in view: position, HP, max HP. */
        monsters: (): ProbeMover[] => {
          const out: ProbeMover[] = [];
          ctx.entities.forEach(EntityKind.Npc, (rec) => {
            if (rec.visual?.kind === 'monster') out.push(describe(rec));
          });
          return out;
        },
        mover: (objId: number): ProbeMover | null => {
          const rec = ctx.entities.get(objId);
          return rec ? describe(rec) : null;
        },
        stats: () => ({ ...rt.stats }),
        recent: (n = 10) => rt.recent.slice(-n).map((r) => ({ ...r, effects: [...r.effects] })),
        projectiles: () => ({
          live: rt.projectiles.live.size,
          ...rt.projectiles.stats,
        }),
        /** The skill runtime: the table, the cool-downs, the cast, the action slot (WP-SKILL-12). */
        skills: () => {
          const now = ctx.now();
          return {
            stats: { ...skills.stats },
            table: skills.table.map((s) => ({ skill: s.skillId, level: s.level })),
            cooldowns: skills.table
              .map((_, slot) => ({ slot, cd: skills.cooldown(slot, now) }))
              .filter((c) => c.cd !== null)
              .map((c) => ({
                slot: c.slot,
                remainMs: Math.round((c.cd?.endMs ?? now) - now),
                totalMs: Math.round((c.cd?.endMs ?? 0) - (c.cd?.startMs ?? 0)),
              })),
            cast: skills.castGauge(now),
            actionSlot: { ...skills.actionSlot },
            telePos: skills.telePos,
            pending: skills.pending,
          };
        },
        buffFx: () => ({ stats: { ...buffFx.stats }, active: Object.fromEntries(buffFx.active()) }),
      });
    }
  },
};

/** What `__flyffDebug.combat.monsters()` / `.mover(id)` report. */
export interface ProbeMover {
  readonly objId: number;
  readonly name: string;
  readonly monster: boolean;
  readonly hp: number;
  readonly maxHp: number;
  readonly peaceful: boolean;
  /** LH position from the render view, `null` when the sim does not know the mover. */
  readonly pos: readonly [number, number, number] | null;
}

export { AF, OBJACT, OBJMSG, NULL_ID } from './constants';
export {
  decideDamageDisplay,
  displayGate,
  type DamageDisplay,
  type DamageEffect,
  type DamageInput,
} from './damageDisplay';
export { ProjectileTracker, type TrackedProjectile } from './projectiles';
export {
  EMPTY_SLOT,
  peekSkillsRuntime,
  SKILL_SLOTS,
  skillsRuntime,
  SUT,
  TICK_MS,
  type ActionSlotState,
  type CastState,
  type CooldownView,
  type FlashState,
  type SkillHooks,
  type SkillRuntime,
  type SkillSlot,
  type SkillStats,
} from './skills';
export { peekBuffFx, type BuffFxRuntime } from './buffs';
export { SkillProps } from './skillProps';
export {
  combatRuntime,
  peekCombatRuntime,
  type CombatHooks,
  type CombatRuntime,
  type CombatStats,
  type DamageRecord,
} from './runtime';
