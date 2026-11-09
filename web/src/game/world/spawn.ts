// `Spawn*` and `DelObj` handlers (Plan/40 section 4.2 SM3-SM4, 5.5, 6.3, 7.4; WP-CLIENT-19; ref/09a section 6.1-6.2).
//
//   phase Core  the entity table (`EntityRecord` + `MoverVisualState`), the own identity (`selfSpawned` -> the state machine's
//               SM3 check) and the player's `selfStore` (`self.ts`)
//   phase Sim   `ADD_MOVER` (+ `SET_ACTIVE` for the own player) and the dirty mark of `SET_PARAMS`; `SET_VISIBLE` is the renderer's
//               (`render/movers/MoverBatcher`: pose evaluation only for the movers it draws, emitted on change)
//
// `DelObj` only marks the entity removed: it is deleted at the end of the drain (G5: later messages of the same frame still
// find it), and the entity table's `onRemove` listener then writes `REMOVE_MOVER`. A spawn of an id that is already known
// updates the record IN PLACE and cancels a pending removal; the sim mover and everything loaded for it stay (ref/09a 6.1: a
// re-appearing object keeps its state, ids are never reused) - but it moves to the position of the new spawn when that is not where
// the sim has it (`snapReappeared`: a teleport that came with the removal in one drain).

import type { ClientContext } from '../../app/context';
import { Phase } from '../../net/registry';
import { OT_MOVER } from '../../gen/defs/define';
import { CK2_KEEP, CK3_HALF } from '../../gen/defs/itemkind';
import type {
  CommonPlace,
  SpawnCtrl,
  SpawnItem,
  SpawnNpc,
  SpawnPlayer,
  SpawnSelf,
} from '../../gen/proto/flyff/v1/world_pb';
import { CTRL_STATE_BITS, KIND } from '../../gen/sim/wire';
import { Prio } from '../../workers/priority';
import { EntityKind, type EntityRecord } from '../entities';
import type { SimMoverKind } from '../moverParams';
import { ctrlWorldMat43, ZERO_AABB } from './ctrlMatrix';
import { simKindOf } from './params';
import type { WorldRuntime } from './runtime';
import { npcKind, npcVisual, playerVisual } from './visual';

const OWNER = 'world.spawn';

/** `true` when a spawn message lacks the parts every spawn has (a bug of the sender, never a crash here). */
function malformed(ctx: ClientContext, what: string, objId: number): void {
  ctx.log.warn('world', `${what} for ${String(objId)} is missing a required part; ignored`);
}

/** The skeleton file of a mover model (`models` bundle `bone`), `null` when unknown. */
export function skeletonPathOf(ctx: ClientContext, moverIndex: number): string | null {
  return ctx.data?.model(OT_MOVER, moverIndex)?.bone ?? null;
}

/** Writes `ADD_MOVER` (+ `SET_ACTIVE`) of a mover record and marks its parameters dirty. */
function addToSim(
  ctx: ClientContext,
  rt: WorldRuntime,
  rec: EntityRecord,
  simKind: SimMoverKind,
  init: {
    readonly pos: { x: number; y: number; z: number };
    readonly angleX10: number;
    readonly state: number;
    readonly stateFlags: number;
    readonly motion: number;
    readonly scale: number;
  },
): void {
  const path = skeletonPathOf(ctx, rec.propId);
  const own = rec.isSelf;
  const skelId = path === null ? 0 : rt.feeder.skeletonId(path, own ? Prio.Critical : Prio.Visible);
  ctx.sim.cmd.addMover({
    id: rec.objId,
    kind: simKind,
    isActive: own,
    moverIndex: rec.propId,
    modelType: OT_MOVER,
    modelIndex: rec.propId,
    skelId,
    pos: [init.pos.x, init.pos.y, init.pos.z],
    angle: init.angleX10 / 10,
    state: init.state,
    stateFlags: init.stateFlags,
    motion: init.motion,
    scale: init.scale,
  });
  rec.simHandle = true;
  rt.params.mark(rec.objId);
  // pose evaluation runs only for movers flagged visible (31 section 15.6): the renderer's mover batcher flags them (a new mover
  // is not visible in the sim until it says so, Plan/41 12.8)
  if (own) {
    ctx.sim.cmd.setActive(rec.objId);
    if (path !== null) rt.feeder.watchOwn(path, rec.propId);
    rt.onOwnAdded(rec);
  }
}

/** How far (m) the sim position of a re-appearing mover may be from the position of its new spawn before the sim mover is moved. */
const REAPPEAR_SNAP_M = 2;

/**
 * A mover that re-appears (a `DelObj` and its spawn in the same drain) keeps its sim mover, but it may have moved while it was gone
 * (a teleport in the middle of the view change, or a view that leaves and returns while several batches queue up): the sim mover
 * jumps to the position the server spawned it at when it is not already there. Without it the object stands at its old place until
 * its next movement message.
 */
function snapReappeared(
  ctx: ClientContext,
  objId: number,
  pos: { x: number; y: number; z: number } | undefined,
): void {
  if (!pos) return;
  const view = ctx.sim.view;
  const at: [number, number, number] = [0, 0, 0];
  if (
    view?.valid === true &&
    view.posOf(objId, at) &&
    Math.hypot(at[0] - pos.x, at[2] - pos.z) <= REAPPEAR_SNAP_M
  )
    return;
  ctx.sim.cmd.setPos(objId, [pos.x, pos.y, pos.z]);
}

type PlayerSpawn = SpawnSelf | SpawnPlayer;

function spawnPlayer(
  ctx: ClientContext,
  objId: number,
  m: PlayerSpawn,
  isSelf: boolean,
): EntityRecord | null {
  if (!m.obj || !m.mover || !m.player) {
    malformed(ctx, isSelf ? 'SpawnSelf' : 'SpawnPlayer', objId);
    return null;
  }
  const visual = playerVisual(
    objId,
    m.obj,
    m.mover,
    m.player,
    'other' in m ? m.other : undefined,
    'self' in m ? m.self : undefined,
  );
  return ctx.entities.spawn(objId, {
    kind: EntityKind.Player,
    propId: m.obj.index,
    spawn: m,
    isSelf,
    idPlayer: m.player.idPlayer,
    visual,
    hp: m.mover.hp,
    state: m.mover.state,
  });
}

function addPlayer(ctx: ClientContext, rt: WorldRuntime, rec: EntityRecord, m: PlayerSpawn): void {
  if (!m.obj || !m.mover) return;
  if (rec.simHandle) {
    // a re-appearing object keeps its sim mover (ref/09a section 6.1)
    if (!rec.isSelf) snapReappeared(ctx, rec.objId, m.obj.pos);
    return;
  }
  addToSim(ctx, rt, rec, KIND.PLAYER, {
    pos: m.obj.pos ?? { x: 0, y: 0, z: 0 },
    angleX10: m.obj.angleX10,
    state: m.mover.state,
    stateFlags: m.mover.stateFlag,
    motion: m.mover.motion,
    scale: rec.visual?.scale ?? 1,
  });
}

function spawnNpc(ctx: ClientContext, objId: number, m: SpawnNpc): EntityRecord | null {
  if (!m.obj || !m.mover) {
    malformed(ctx, 'SpawnNpc', objId);
    return null;
  }
  const visual = npcVisual(objId, m.obj, m.mover, m.npc, ctx.data);
  // a monster's maximum HP is `dwAddHp` of its property row (`CMover::GetMaxOriginHitPoint`, ref/10 section 5): the HP bar of
  // the target frame needs it, the spawn only carries the current HP
  const addHp = ctx.data?.mover(m.obj.index)?.add_hp ?? 0;
  return ctx.entities.spawn(objId, {
    kind: EntityKind.Npc,
    propId: m.obj.index,
    spawn: m,
    visual,
    hp: m.mover.hp,
    ...(addHp > 0 ? { maxHp: Math.max(addHp, m.mover.hp) } : {}),
    state: m.mover.state,
  });
}

function addNpc(ctx: ClientContext, rt: WorldRuntime, rec: EntityRecord, m: SpawnNpc): void {
  if (!m.obj || !m.mover) return;
  if (rec.simHandle) {
    snapReappeared(ctx, rec.objId, m.obj.pos);
    return;
  }
  const kind = npcKind(ctx.data?.mover(rec.propId)?.ai, m.npc?.characterKey ?? '').sim;
  const simKind = kind === 'pet' ? KIND.PET : kind === 'npc' ? KIND.NPC : KIND.MONSTER;
  addToSim(ctx, rt, rec, simKind, {
    pos: m.obj.pos ?? { x: 0, y: 0, z: 0 },
    angleX10: m.obj.angleX10,
    state: m.mover.state,
    stateFlags: m.mover.stateFlag,
    motion: m.mover.motion,
    scale: rec.visual?.scale ?? 1,
  });
}

/**
 * `CTRL_ADD` of a ctrl (Plan/31 section 17.4): the world matrix of its quantised spawn, the model key (the ctrl index) and the spawn
 * order. The sim registers the dynamic collider from the `FLYK` model of the ctrl and creates the animation entry when the model
 * animates; a ctrl without a model is counted by the sim and has no collider.
 */
function addCtrl(ctx: ClientContext, rec: EntityRecord, m: SpawnCtrl): void {
  const obj = m.obj;
  if (!obj || rec.simHandle) return;
  const p = obj.pos ?? { x: 0, y: 0, z: 0 };
  ctx.sim.cmd.ctrlAdd({
    id: rec.objId,
    modelKey: obj.index,
    world: ctrlWorldMat43([p.x, p.y, p.z], obj.angleX10, obj.scaleX100),
    aabb: ZERO_AABB,
    seq: rec.objId,
  });
  rec.simHandle = true;
}

/** The `CTRL_STATE` loop bits of a ctrl: `CK2_KEEP` re-arms it, `CK3_HALF` pauses it at the middle of its animation. */
function ctrlStateBits(ctx: ClientContext, ctrlId: number): number {
  const prop = ctx.data?.ctrl(ctrlId);
  return (
    (prop?.ctrl_kind2 === CK2_KEEP ? CTRL_STATE_BITS.KEEP : 0) |
    (prop?.ctrl_kind3 === CK3_HALF ? CTRL_STATE_BITS.HALF : 0)
  );
}

/** The entity record the Core handler made for `objId` (the Sim handler of the same message reads it). */
function recordOf(ctx: ClientContext, objId: number): EntityRecord | undefined {
  return ctx.entities.get(objId);
}

export function registerSpawnHandlers(ctx: ClientContext, rt: WorldRuntime): void {
  const { registry } = ctx.net;

  // ---- phase Core: entity table and identity ---------------------------------------------------------------
  registry.on('spawnSelf', Phase.Core, OWNER, (objId, m) => {
    const rec = spawnPlayer(ctx, objId, m, true);
    if (!rec) return;
    // SM3: the first SpawnSelf of a join must be the selected character, else one resync (then fatal)
    ctx.machine.dispatch({ type: 'selfSpawned', idPlayer: rec.idPlayer ?? 0, now: ctx.now() });
  });
  registry.on('spawnPlayer', Phase.Core, OWNER, (objId, m) => {
    spawnPlayer(ctx, objId, m, false);
  });
  registry.on('spawnNpc', Phase.Core, OWNER, (objId, m) => {
    spawnNpc(ctx, objId, m);
  });
  registry.on('spawnItem', Phase.Core, OWNER, (objId, m: SpawnItem) => {
    if (!m.obj) {
      malformed(ctx, 'SpawnItem', objId);
      return;
    }
    ctx.entities.spawn(objId, { kind: EntityKind.Item, propId: m.obj.index, spawn: m });
  });
  registry.on('spawnCtrl', Phase.Core, OWNER, (objId, m: SpawnCtrl) => {
    if (!m.obj) {
      malformed(ctx, 'SpawnCtrl', objId);
      return;
    }
    ctx.entities.spawn(objId, { kind: EntityKind.Ctrl, propId: m.obj.index, spawn: m });
  });
  registry.on('delObj', Phase.Core, OWNER, (objId) => {
    ctx.entities.markRemoved(objId);
  });

  // ---- phase Sim: the sim movers ------------------------------------------------------------------------------
  registry.on('spawnSelf', Phase.Sim, OWNER, (objId, m) => {
    const rec = recordOf(ctx, objId);
    if (rec) addPlayer(ctx, rt, rec, m);
  });
  registry.on('spawnPlayer', Phase.Sim, OWNER, (objId, m) => {
    const rec = recordOf(ctx, objId);
    if (rec) addPlayer(ctx, rt, rec, m);
  });
  registry.on('spawnNpc', Phase.Sim, OWNER, (objId, m) => {
    const rec = recordOf(ctx, objId);
    if (rec) addNpc(ctx, rt, rec, m);
  });
  registry.on('spawnCtrl', Phase.Sim, OWNER, (objId, m: SpawnCtrl) => {
    const rec = recordOf(ctx, objId);
    if (rec) addCtrl(ctx, rec, m);
  });
  // `COMMONPLACE_ACTION` (`CCommonCtrl::SetActionPlay`): the ctrl plays on from where it holds, from the server frame of the packet;
  // the fade (`ALPHA`) and the effect are the renderer's (`game/fx`). Ground items are not simulated.
  registry.on('commonPlace', Phase.Sim, OWNER, (objId, m: CommonPlace, mctx) => {
    if (m.kind !== 0) return;
    const rec = recordOf(ctx, objId);
    if (rec?.kind !== EntityKind.Ctrl || !rec.simHandle) return;
    ctx.sim.cmd.ctrlState(objId, mctx.serverFrame, -1, ctrlStateBits(ctx, rec.propId));
  });

  // ---- removal: the sweep at the end of the drain writes REMOVE_MOVER (G5) -----------------------------------------
  ctx.entities.onRemove((rec, reason) => {
    rt.params.forget(rec.objId);
    if (reason === 'sweep' && rec.simHandle) {
      if (rec.kind === EntityKind.Ctrl) ctx.sim.cmd.ctrlRemove(rec.objId);
      else ctx.sim.cmd.removeMover(rec.objId);
    }
    rt.onEntityRemoved(rec, reason);
  });

  // the parameters of the movers spawned in this drain are written once, after the last frame (cap: one per iteration)
  registry.onDrainEnd(OWNER, () => {
    rt.params.flush();
  });
}

export { simKindOf };
