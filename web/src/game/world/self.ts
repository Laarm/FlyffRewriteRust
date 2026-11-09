// The own character in `selfStore` (Plan/40 section 6.2.2, 6.4; the field contract is 45 section 2.3; WP-CLIENT-29): the
// handlers of the messages that describe the own character reduce into the store with the pure functions of
// `stores/selfReducers.ts` (one `setState` per message, ST3/ST4).
//
//   SpawnSelf                the whole state; the live record starts from it
//   SetPointParam            HP / MP / FP into the live record (the publisher copies them at 10 Hz, ST5), the purse into the store
//   SetExperience            EXP into the live record, level / skill points / death penalty memory into the store
//   SetStatLevel, SetState, SetGrowthLearningPoint   the base stats and the remaining stat points
//   JobChanged, ModifyMode, SetFame, SetFxp, PkRelation, SmMode, SmModeAll, SetLevel
//
// High-frequency values (hp, mp, fp, exp) are NOT written into the store by the handlers (ST5): they go into the `SelfLive`
// record and the publishers copy them (`selfGauges`, `selfExp` at 10 Hz, `selfPos` at 4 Hz). The messages about OTHER movers
// that share a type with the own ones (`ModifyMode`, `PkRelation`, `SetPointParam` of a monster) update the entity record
// (`visual.mode`, `visual.pk`, `hp`) instead. A message whose header is 0 or the own id is the own character's.

import type { ClientContext } from '../../app/context';
import { Phase } from '../../net/registry';
import {
  applyJobChanged,
  applyModifyMode,
  applyPkRelation,
  applySetExperience,
  applySetFame,
  applySetFxp,
  applySetGrowthLearningPoint,
  applySetLevel,
  applySetPointParam,
  applySetState,
  applySetStatLevel,
  applySmMode,
  applySmModeAll,
  applySpawnSelf,
  liveFromSpawn,
  newSelfLive,
  POINT_DST,
  type SelfLive,
} from '../../stores/selfReducers';
import type { SelfState } from '../../stores/self';
import { peekWorldRuntime } from './runtime';

export { newSelfLive, type SelfLive };

/** The `SelfState` a `SpawnSelf` describes (kept under its first-cut name; the reducer is `applySpawnSelf`). */
export const selfStateFromSpawn = applySpawnSelf;

const OWNER = 'world.self';

export function registerSelfHandlers(ctx: ClientContext, live: SelfLive): void {
  const { registry } = ctx.net;
  const { selfStore } = ctx.stores;
  /** The header names the own character (`0` = self, or the own object id). */
  const isSelf = (objId: number): boolean =>
    objId === 0 || (live.objId !== 0 && objId === live.objId);
  const patch = (p: Partial<SelfState> | null): void => {
    if (p !== null) selfStore.setState(p);
  };
  registry.on('spawnSelf', Phase.Core, OWNER, (objId, m) => {
    const state = applySpawnSelf(m, objId, ctx.stores.worldStore.getState().worldId);
    if (!state) return;
    selfStore.setState(state);
    liveFromSpawn(live, state);
  });

  // absolute values: the original `SetPointParam(dst, value)` (ref/09a 5.6); `DST_GOLD` is the own character only (11a)
  registry.on('setPointParam', Phase.Core, OWNER, (objId, m) => {
    if (isSelf(objId)) {
      patch(applySetPointParam(live, selfStore.getState(), m.dst, m.value));
      if (m.dst === POINT_DST.HP) {
        const rec = ctx.entities.self();
        if (rec) rec.hp = m.value;
      }
      return;
    }
    // another mover: its HP (a heal, a leash reset); the sim learns "alive" from the next parameter record
    if (m.dst !== POINT_DST.HP) return;
    const rec = ctx.entities.get(objId);
    if (!rec) return;
    const wasDown = rec.hp <= 0;
    rec.hp = m.value;
    if (wasDown !== m.value <= 0) peekWorldRuntime(ctx)?.params.mark(objId);
  });

  registry.on('setExperience', Phase.Core, OWNER, (_objId, m) => {
    patch(applySetExperience(live, m));
  });

  registry.on('setLevel', Phase.Core, OWNER, (objId, m) => {
    if (!isSelf(objId)) return;
    patch(applySetLevel(m.level));
  });

  registry.on('setStatLevel', Phase.Core, OWNER, (objId, m) => {
    if (!isSelf(objId)) return;
    patch(applySetStatLevel(selfStore.getState(), m));
  });
  registry.on('setState', Phase.Core, OWNER, (_objId, m) => {
    patch(applySetState(m));
  });
  registry.on('setGrowthLearningPoint', Phase.Core, OWNER, (_objId, m) => {
    patch(applySetGrowthLearningPoint(m));
  });
  registry.on('jobChanged', Phase.Core, OWNER, (objId, m) => {
    if (!isSelf(objId)) return;
    patch(applyJobChanged(m));
  });
  registry.on('setFame', Phase.Core, OWNER, (objId, m) => {
    if (isSelf(objId)) patch(applySetFame(m));
  });
  registry.on('setFxp', Phase.Core, OWNER, (_objId, m) => {
    patch(applySetFxp(m));
  });
  registry.on('smMode', Phase.Core, OWNER, (_objId, m) => {
    patch(applySmMode(selfStore.getState(), m));
  });
  registry.on('smModeAll', Phase.Core, OWNER, (_objId, m) => {
    patch(applySmModeAll(m));
  });

  // the mode word and the PK flags are visible on every player (name colours, `NO_ATTACK_MODE` in the sim parameters)
  registry.on('modifyMode', Phase.Core, OWNER, (objId, m) => {
    const id = objId === 0 ? live.objId : objId;
    const rec = ctx.entities.get(id);
    if (rec?.visual) {
      rec.visual.mode = m.mode;
      peekWorldRuntime(ctx)?.params.mark(id);
    }
    if (isSelf(objId)) patch(applyModifyMode(m));
  });
  registry.on('pkRelation', Phase.Core, OWNER, (objId, m) => {
    const id = objId === 0 ? live.objId : objId;
    const rec = ctx.entities.get(id);
    const sub = m.sub;
    if (rec?.visual) {
      if (sub.case === 'pink') rec.visual.pk = { ...rec.visual.pk, pink: sub.value.pink !== 0 };
      else if (sub.case === 'propensity') {
        rec.visual.pk = { ...rec.visual.pk, chaotic: sub.value.propensity > 0 };
      }
    }
    if (isSelf(objId)) patch(applyPkRelation(selfStore.getState(), m));
  });

  // `selfGauges` and `selfExp` at 10 Hz, `selfPos` at 4 Hz (Plan/40 section 6.4); each writes only what changed. A hundred
  // `SetPointParam` of one drain cost ONE store update at the next tick (WP-CLIENT-29 acceptance).
  ctx.publisher.register('selfGauges', 10, () => {
    const s = selfStore.getState();
    if (s.hp !== live.hp || s.mp !== live.mp || s.fp !== live.fp) {
      selfStore.setState({ hp: live.hp, mp: live.mp, fp: live.fp });
    }
  });
  ctx.publisher.register('selfExp', 10, () => {
    const s = selfStore.getState();
    if (s.exp !== live.exp) selfStore.setState({ exp: live.exp });
    live.dirty = false;
  });
  ctx.publisher.register('selfPos', 4, () => {
    const view = ctx.sim.view;
    const id = live.objId;
    if (!view || id === 0) return;
    const out: [number, number, number] = [0, 0, 0];
    if (!view.posOf(id, out)) return;
    const s = selfStore.getState();
    const [x, y, z] = s.pos;
    if (x !== out[0] || y !== out[1] || z !== out[2]) selfStore.setState({ pos: out });
  });
}
