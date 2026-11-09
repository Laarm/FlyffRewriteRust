// The combat runtime of one `ClientContext` (Plan/17 section 11, Plan/40 section 7.6; WP-COMBAT-10): the objects the combat
// handlers share - the projectile bookkeeping, the hooks other documents plug in (the item cast cancel, the personal-shop
// close, the options), the own death/revival subscriptions - and the counters of the probe (`__flyffDebug.combat`).

import type { ClientContext } from '../../app/context';
import type { RenderFx } from '../../render/fx';
import { ProjectileTracker } from './projectiles';

/** The integration points of the windows and options that the combat runtime consults; every member is optional. */
export interface CombatHooks {
  /** The own player is casting an item (`STATE_BASEMOTION_MODE`). */
  baseMotionActive?(): boolean;
  /** `SendStateModeCancel(STATE_BASEMOTION_MODE, STATEMODE_BASEMOTION_CANCEL)`; default: send the `StateModeReq`. */
  cancelBaseMotion?(): void;
  /** The own personal-shop window is open: a hit closes it and the inventory (`OnDamage`, ref/09a 6.5). */
  closeVendorOnHit?(): void;
  /** The option "show all damage" (`g_Option.m_bDamageRender`). */
  showAllDamage?(): boolean;
  /** The viewer's authority (`AUTH_*` letter); default: the self store. */
  authority?(): number;
}

/** One decided `Damage` (the probe and the tests read the ring). */
export interface DamageRecord {
  readonly victim: number;
  readonly attacker: number;
  readonly damage: number;
  readonly atkFlags: number;
  readonly hpAfter: number;
  readonly shown: boolean;
  /** Effect ids created (`XI_HIT_*`). */
  readonly effects: readonly number[];
  /** The number floated over the victim, `null` for none. */
  readonly number: number | null;
}

export interface CombatStats {
  damages: number;
  damagesShown: number;
  numbers: number;
  misses: number;
  criticals: number;
  blocks: number;
  parries: number;
  deaths: number;
  ownDeaths: number;
  revivals: number;
  attackNtfs: number;
  projectileSpawns: number;
  projectileHits: number;
  projectileExpired: number;
  stateModeCancels: number;
}

export interface CombatRuntime {
  readonly projectiles: ProjectileTracker;
  readonly hooks: CombatHooks;
  readonly stats: CombatStats;
  /** The last decided hits, newest last (at most {@link DAMAGE_RING}). */
  readonly recent: DamageRecord[];
  /** The render effects of this moment (`undefined` until the renderer exists). */
  fx(): RenderFx | undefined;
  /** Subscribes to the own death / revival; returns the unsubscribe function. */
  onOwnDeath(fn: (attacker: number) => void): () => void;
  onOwnRevived(fn: () => void): () => void;
  emitOwnDeath(attacker: number): void;
  emitOwnRevived(): void;
}

/** Hits kept for the probe. */
export const DAMAGE_RING = 50;

const runtimes = new WeakMap<ClientContext, CombatRuntime>();

export function peekCombatRuntime(ctx: ClientContext): CombatRuntime | undefined {
  return runtimes.get(ctx);
}

export function combatRuntime(ctx: ClientContext): CombatRuntime {
  const known = runtimes.get(ctx);
  if (known) return known;
  const deaths = new Set<(attacker: number) => void>();
  const revivals = new Set<() => void>();
  const rt: CombatRuntime = {
    projectiles: new ProjectileTracker(() => ctx.render?.fx),
    hooks: {},
    stats: {
      damages: 0,
      damagesShown: 0,
      numbers: 0,
      misses: 0,
      criticals: 0,
      blocks: 0,
      parries: 0,
      deaths: 0,
      ownDeaths: 0,
      revivals: 0,
      attackNtfs: 0,
      projectileSpawns: 0,
      projectileHits: 0,
      projectileExpired: 0,
      stateModeCancels: 0,
    },
    recent: [],
    fx: () => ctx.render?.fx,
    onOwnDeath(fn) {
      deaths.add(fn);
      return () => {
        deaths.delete(fn);
      };
    },
    onOwnRevived(fn) {
      revivals.add(fn);
      return () => {
        revivals.delete(fn);
      };
    },
    emitOwnDeath(attacker) {
      for (const fn of [...deaths]) {
        try {
          fn(attacker);
        } catch (error) {
          ctx.log.error('combat', `own death listener: ${String(error)}`);
        }
      }
    },
    emitOwnRevived() {
      for (const fn of [...revivals]) {
        try {
          fn();
        } catch (error) {
          ctx.log.error('combat', `revival listener: ${String(error)}`);
        }
      }
    },
  };
  runtimes.set(ctx, rt);
  return rt;
}
