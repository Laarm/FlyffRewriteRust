// The rules runtime of one `ClientContext` (Plan/40 section 7.10, Plan/16 section 18.4/18.6; WP-CLIENT-30): the `RulesContext`
// facade (`ctx.rules`, built as soon as the four bundles it needs are in), the DST replica sync of every network drain
// (`sync.ts`), the DST-derived `SET_PARAMS` fields of every mover, and `derived_stats` of the own character (at most 10 Hz,
// when its inputs changed). Everything that costs a boundary call is an R item of the sim bridge (every queued item runs at the
// next `runRules`, FIFO; the R counter is telemetry, Plan/40 section 7.3) or a U call of a user-driven moment (the EXP percentage).

import type { ClientContext } from '../../app/context';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';
import type { SpawnNpc, SpawnPlayer, SpawnSelf } from '../../gen/proto/flyff/v1/world_pb';
import type { DrainRow, DstTriple, ParamRowIn, RulesCfgJs, RulesFacade } from '../../rules';
import { createRulesContext } from '../../rules';
import { Phase } from '../../net/registry';
import type { ResettableScope } from '../../stores/registry';
import { EntityKind, type EntityRecord } from '../entities';
import type { RulesItem, SimBridge } from '../ports';
import { worldRuntime } from '../world/runtime';
import { flattenDerived, statInputOf, weaponItemOf } from './selfStats';
import { RulesSync, type RulesPort } from './sync';

const OWNER = 'rules';

/** `derived_stats` runs at most this often (Plan/40 section 7.10: every 100 ms and only when its inputs changed). */
export const DERIVED_PERIOD_MS = 100;

/** Builds the facade; `null` while the bundles or the module are not there yet (asked again). */
export type RulesFactory = (ctx: ClientContext) => RulesFacade | null;

export interface RulesRuntimeOptions {
  /** Builds the facade (default: the four retained bundles through `createRulesContext`); tests inject a fake. */
  readonly factory?: RulesFactory;
}

export interface RulesRuntime {
  readonly sync: RulesSync;
  /** The facade, built on first need; `null` while the bundles or the wasm module are missing. */
  rules(): RulesFacade | null;
  /** The base stats, level, job, equipment or DST replica of the own character changed: the derived numbers are recomputed. */
  markSelfStatsDirty(): void;
  /** The attack-speed inputs of a mover changed: its derived row is fetched with the next sync. */
  markMoverDirty(id: number): void;
  /** Replaces the facade factory (tests, dev surfaces) and forgets an earlier failure. */
  setFactory(factory: RulesFactory): void;
  /** Counters for the probe and the tests. */
  readonly stats: {
    derivedRuns: number;
    expPercentRuns: number;
    created: boolean;
    unavailable: string | null;
  };
  /** Hands the queued work to the bridge, which runs every queued item. Called by the drain-end hook and every step. */
  pump(): void;
  /** `session` / `world` scope reset. */
  onScopeReset(scope: ResettableScope): void;
}

const runtimes = new WeakMap<ClientContext, RulesRuntime>();

export function peekRulesRuntime(ctx: ClientContext): RulesRuntime | undefined {
  return runtimes.get(ctx);
}

export function rulesRuntime(ctx: ClientContext, options: RulesRuntimeOptions = {}): RulesRuntime {
  const known = runtimes.get(ctx);
  if (known) return known;
  const rt = createRulesRuntime(ctx, options);
  runtimes.set(ctx, rt);
  return rt;
}

/** The `RulesContext` switches from the typed client features (`features.*`, `gameplay.*`). */
export function rulesConfigOf(config: ClientFeatureConfig): RulesCfgJs {
  const f = config.features;
  const g = config.gameplay;
  return {
    bowdmgPropjob: f.bowdmg_propjob,
    wandAutoattacks: f.wand_autoattacks,
    betterArmorBonus: f.better_armor_bonus,
    autoSkillMax: f.auto_skill_max,
    fixGetHpMpFpPoint: f.fix_get_hp_mp_fp_point,
    removeMaxRecoverRate: f.remove_max_recover_rate,
    setMaxSpeed: g.set_max_speed,
    setMaxCast: g.set_max_cast,
    setMaxCrit: g.set_max_crit,
    maxBlockValue: g.max_block_value,
    maxBlockValueGw: g.max_block_value_gw,
  };
}

/** The default factory: the four retained bundle files through the wasm `RulesContext`. */
function defaultFactory(ctx: ClientContext): RulesFacade | null {
  const raw = (name: 'tables' | 'items' | 'item-ext' | 'skills'): Uint8Array | null => {
    const b = ctx.bundles.raw(name);
    return b ? new Uint8Array(b) : null;
  };
  const tables = raw('tables');
  const items = raw('items');
  const itemExt = raw('item-ext');
  const skills = raw('skills');
  if (!tables || !items || !itemExt || !skills) return null;
  return createRulesContext({ tables, items, itemExt, skills }, rulesConfigOf(ctx.features));
}

function dstTriplesOf(m: SpawnSelf | SpawnPlayer | SpawnNpc): readonly DstTriple[] {
  const list = m.mover?.dstParams ?? [];
  return list.map((d) => ({ dst: d.dst, adj: d.adj, chg: d.chg }));
}

function createRulesRuntime(ctx: ClientContext, options: RulesRuntimeOptions): RulesRuntime {
  let factory: RulesFactory = options.factory ?? defaultFactory;
  const world = worldRuntime(ctx);
  const { selfStore } = ctx.stores;
  const stats = {
    derivedRuns: 0,
    expPercentRuns: 0,
    created: false,
    unavailable: null as string | null,
  };
  let failed = false;
  let selfDirty = false;
  /** The (level, exp) the last percentage was computed for. */
  let pctLevel = -1;
  let pctExp = -1;
  let derivedQueued = false;
  let lastDerivedAt = Number.NEGATIVE_INFINITY;

  // a bridge without the rules queue (a partial fake in a test) runs the item at once
  const enqueueRules = (item: RulesItem): void => {
    const sim = ctx.sim as Partial<SimBridge>;
    if (typeof sim.enqueueRules === 'function') sim.enqueueRules(item);
    else item.run();
  };
  const runRules = (): void => {
    const sim = ctx.sim as Partial<SimBridge>;
    if (typeof sim.runRules === 'function') sim.runRules();
  };

  const rules = (): RulesFacade | null => {
    if (ctx.rules) return ctx.rules as RulesFacade;
    if (failed) return null;
    let made: RulesFacade | null;
    try {
      made = factory(ctx);
    } catch (error) {
      failed = true;
      stats.unavailable = error instanceof Error ? error.message : String(error);
      ctx.log.error('rules', `the rules context could not be built: ${stats.unavailable}`);
      return null;
    }
    if (!made) return null; // bundles or module not there yet: asked again later
    ctx.rules = made;
    stats.created = true;
    queueMicrotask(() => {
      sync.onRulesReady();
      selfDirty = true;
      pctLevel = -1;
    });
    return made;
  };

  const port = (): RulesPort | null => rules();

  /** The attack-speed inputs of a mover (`[obj, job, level, base_dex, weapon_item]`). */
  const rowOf = (obj: number): ParamRowIn | null => {
    const rec = ctx.entities.get(obj);
    if (!rec || rec.removed) return null;
    return rowOfRecord(rec);
  };
  const rowOfRecord = (rec: EntityRecord): ParamRowIn | null => {
    if (rec.kind === EntityKind.Player) {
      const s = selfStore.getState();
      const spawn = rec.spawn as SpawnSelf | SpawnPlayer;
      const p = spawn.player;
      if (rec.isSelf) {
        return {
          obj: rec.objId,
          job: s.job,
          level: s.level,
          baseDex: s.base.dex,
          weaponItem: weaponItemOf(rec.visual),
        };
      }
      return {
        obj: rec.objId,
        job: p?.job ?? -1,
        level: p?.level ?? 1,
        baseDex: p?.dex ?? 0,
        weaponItem: weaponItemOf(rec.visual),
      };
    }
    if (rec.kind === EntityKind.Npc) {
      return {
        obj: rec.objId,
        job: -1,
        level: ctx.data?.mover(rec.propId)?.level ?? 1,
        baseDex: 0,
        weaponItem: 0,
      };
    }
    return null;
  };

  const sync: RulesSync = new RulesSync({
    rules: port,
    enqueue: (item) => {
      enqueueRules(item);
    },
    rowOf,
    apply: (obj: number, row: DrainRow) => {
      world.params.setDerived(obj, row);
    },
    applied: () => {
      world.params.flush();
    },
    log: ctx.log,
  });

  const markSelfStatsDirty = (): void => {
    selfDirty = true;
    const id = world.live.objId;
    if (id !== 0) sync.markDirty(id);
  };

  /** Queues the `derived_stats` item when the inputs changed and the 100 ms period allows. */
  const scheduleDerived = (now: number): void => {
    if (!selfDirty || derivedQueued) return;
    if (now - lastDerivedAt < DERIVED_PERIOD_MS) return;
    if (!rules()) return;
    derivedQueued = true;
    selfDirty = false;
    enqueueRules({
      name: 'derived_stats',
      urgentAfterMs: DERIVED_PERIOD_MS,
      run: () => {
        derivedQueued = false;
        lastDerivedAt = ctx.now();
        const r = rules();
        const rec = ctx.entities.self();
        if (!r || !rec) return;
        const s = selfStore.getState();
        const input = statInputOf(s, rec.visual);
        const d = r.derivedStats(rec.objId, input);
        stats.derivedRuns += 1;
        rec.maxHp = d.maxHp;
        selfStore.setState({
          maxHp: d.maxHp,
          maxMp: d.maxMp,
          maxFp: d.maxFp,
          // with the inputs it belongs to: the character window waits for a record that matches the base stats it shows
          derived: flattenDerived(d, input),
        });
      },
    });
  };

  const pump = (): void => {
    if (sync.pending) sync.endDrain();
    scheduleDerived(ctx.now());
    runRules();
  };

  const rt: RulesRuntime = {
    sync,
    rules,
    markSelfStatsDirty,
    markMoverDirty(id) {
      sync.markDirty(id);
    },
    stats,
    pump,
    setFactory(f) {
      factory = f;
      failed = false;
      stats.unavailable = null;
    },
    onScopeReset(scope) {
      if (scope === 'session' || scope === 'world') {
        sync.reset();
        selfDirty = false;
        pctLevel = -1;
      }
    },
  };

  // ---- the experience percentage: a U call, at most 10 Hz, only when EXP or the level changed ------------------------------
  ctx.publisher.register('selfExpPercent', 10, () => {
    const s = selfStore.getState();
    if (s.level === pctLevel && s.exp === pctExp) return;
    const r = rules();
    if (!r) return;
    pctLevel = s.level;
    pctExp = s.exp;
    const pct = r.expPercentX100(s.level, s.exp);
    stats.expPercentRuns += 1;
    if (pct !== s.expPercentX100) selfStore.setState({ expPercentX100: pct });
  });

  return rt;
}

/** The handler module of the rules runtime (registered in `CORE_MODULES` before the world modules, so that its drain-end hook runs first). */
export const rulesModule = {
  id: 'rules',
  register(ctx: ClientContext): void {
    const rt = rulesRuntime(ctx);
    const { registry } = ctx.net;
    const selfId = (): number => ctx.entities.self()?.objId ?? 0;
    const touched = (obj: number): void => {
      if (obj !== 0 && obj === selfId()) rt.markSelfStatsDirty();
    };

    // spawn snapshots (phase Sim: the entity exists, Core created it)
    registry.on('spawnSelf', Phase.Sim, OWNER, (objId, m) => {
      rt.sync.addSnapshot(objId, dstTriplesOf(m));
    });
    registry.on('spawnPlayer', Phase.Sim, OWNER, (objId, m) => {
      rt.sync.addSnapshot(objId, dstTriplesOf(m));
    });
    // a monster without any DST value keeps the default parameters: its snapshot is empty and it gets no row
    registry.on('spawnNpc', Phase.Sim, OWNER, (objId, m) => {
      const triples = dstTriplesOf(m);
      rt.sync.addSnapshot(objId, triples, triples.length > 0);
    });

    // the base values of the own character changed: the derived numbers (max HP, ...) and its attack speed are recomputed
    for (const c of [
      'spawnSelf',
      'setExperience',
      'setLevel',
      'setStatLevel',
      'setState',
      'jobChanged',
    ] as const) {
      registry.on(c, Phase.Sim, OWNER, (objId) => {
        if (c === 'spawnSelf' || objId === 0 || objId === selfId()) rt.markSelfStatsDirty();
      });
    }

    // the three DST messages replay on the replica (Plan/16 section 4.7)
    registry.on('setDestParam', Phase.Sim, OWNER, (objId, m) => {
      rt.sync.addOp(objId, 0, m.dst, m.adj, m.chg);
      touched(objId);
    });
    registry.on('resetDestParam', Phase.Sim, OWNER, (objId, m) => {
      rt.sync.addOp(objId, 1, m.dst, m.adj, 0);
      touched(objId);
    });
    registry.on('resetDestParamSync', Phase.Sim, OWNER, (objId, m) => {
      rt.sync.addOp(objId, 2, m.dst, m.adj, m.total);
      touched(objId);
    });

    // equipment changed (the visual state is updated by the item handlers of the Feature phase, so look afterwards): the weapon
    // is an input of the attack speed
    registry.on('doEquipNtf', Phase.Fx, OWNER, (objId) => {
      const id = objId === 0 ? selfId() : objId;
      rt.markMoverDirty(id);
      touched(id);
    });

    // despawn: the replica goes
    ctx.entities.onRemove((rec) => {
      rt.sync.addForget(rec.objId);
    });

    // a world change in the middle of a drain (and a join batch): what this drain collected belongs to the old world, the replicas
    // start again from the batch's snapshots. After the Core handlers of the world module (they clear the entity table)
    registry.on('replace', Phase.Sim, OWNER, () => {
      rt.sync.reset();
    });
    registry.on('worldReadInfo', Phase.Sim, OWNER, () => {
      rt.sync.reset();
    });
    // world change and re-attach: every replica starts again from the join batch (Plan/40 section 7.10)
    ctx.machine.subscribe((screen, previous) => {
      if (screen.s === 'Loading' && previous.s !== 'Loading') rt.sync.reset();
      else if (screen.s === 'Login' || screen.s === 'CharSelect' || screen.s === 'Leaving') {
        if (previous.s !== screen.s) rt.onScopeReset('world');
      }
    });

    registry.onDrainEnd(OWNER, () => {
      rt.pump();
    });
    ctx.tickSystems.register(OWNER, () => {
      rt.pump();
    });

    if (!ctx.probe.sections().includes('rules')) {
      ctx.probe.extend('rules', {
        stats: () => ({ ...rt.stats, sync: { ...rt.sync.stats }, ready: ctx.rules !== null }),
      });
    }
  },
};
