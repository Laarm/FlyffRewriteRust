// The local system pets (Plan/40 §7.9, Plan/18 §13, F-184; WP-CLIENT-31; `src:_Common/Mover.cpp:11112-11200` `ProcessPet`): the server has no mover for a
// summoned system pet (an egg or a hatched pet is an item), so every client builds a LOCAL one for each player with an active pet:
//
//   * an `EntityKind.LocalPet` record with a synthetic `SpawnNpc` (model, name, scale) so that the renderer draws it like any other mover,
//   * `ADD_MOVER { kind: PET }` and (through the params manager) `SET_PARAMS { ai: AII_EGG, is_pet, pet_owner }`; the sim drives it
//     (`flyff_sim::client::local_pet`: it walks to the nearer side point of its owner), so no movement code exists here (B1),
//   * a local id from `0xFF000000` upward. Nothing about a local pet is ever sent to the server (the sender has no way to name it) and it is
//     never pickable or targetable (`entityInfoOf` answers `null` for it).
//
// The pet ends with `PetReleaseNtf`, a level change replaces it (`PetLevelup`, `Pet` with a level flag: `m_pet.GetObj()->Delete()`, the next
// `ProcessPet` builds the new model), and it goes with its owner (`DelObj`, a world change clears the entity table). `features.hide_pets` /
// the `OffPets` option need no code here: the renderer, the nameplates and the picker read the `ai` of the mover row (`AII_EGG`).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { OT_MOVER } from '../../gen/defs/define';
import {
  MoverCommonSchema,
  NpcPartSchema,
  ObjCommonSchema,
} from '../../gen/proto/flyff/v1/mover_types_pb';
import { SpawnNpcSchema } from '../../gen/proto/flyff/v1/world_pb';
import { KIND } from '../../gen/sim/wire';
import { Prio } from '../../workers/priority';
import { EntityKind, type EntityRecord } from '../entities';
import { skeletonPathOf } from '../world/spawn';
import { worldRuntime } from '../world/runtime';
import { npcVisual } from '../world/visual';

/** The first local object id; the server never issues an id from this range (Plan/40 §7.9, Open question 7). */
export const LOCAL_ID_BASE = 0xff00_0000;
/** The last local id (`0xFFFFFFFF` is `NULL_ID`). */
export const LOCAL_ID_END = 0xffff_fffe;
/** Specs of owners that are not in view yet (a bound, not a limit anyone reaches). */
const MAX_WAITING = 256;

/** What the pet of one owner looks like. */
export interface LocalPetSpec {
  /** `MoverProp` id of its model (`CPet::GetIndex`). */
  readonly moverIndex: number;
  /** `PL_*` (the label of the status window and the sparkle of an S pet). */
  readonly level: number;
  /** `CPet::GetName`, `''` = the model's name. */
  readonly name: string;
}

export interface EggPetStats {
  created: number;
  replaced: number;
  removed: number;
  /** Specs waiting for an owner that is not in view yet. */
  pending: number;
}

export interface EggPets {
  /**
   * The owner has an active system pet of `spec`. The local pet is created now when the owner is in view; else the spec waits for the
   * owner's spawn. A pet of another model replaces the old one (`PetLevelup`); the same model only takes the new name.
   */
  summon(ownerId: number, spec: LocalPetSpec): void;
  /** The pet ended (`PetReleaseNtf`): its local mover goes. A waiting spec is dropped. */
  release(ownerId: number): void;
  /** The owner left the view (`DelObj`): the pet goes with it; the spec of a re-appearing owner is kept by the caller. */
  ownerGone(ownerId: number): void;
  /** The owner came into view (`Spawn*`): a waiting spec turns into the pet. */
  ownerArrived(ownerId: number): void;
  /** The local object id of the pet of `ownerId`. */
  localIdOf(ownerId: number): number | undefined;
  /** The spec the owner's pet was built from (also while it waits). */
  specOf(ownerId: number): LocalPetSpec | undefined;
  /** `true` for an id of the local range (such an id is never sent anywhere). */
  isLocal(objId: number): boolean;
  readonly stats: EggPetStats;
  /** Forgets everything (the entity table was cleared). */
  reset(): void;
}

const pets = new WeakMap<ClientContext, EggPets>();

/** The local pets of a context, `undefined` before `createEggPets`. */
export function eggPetsOf(ctx: ClientContext): EggPets | undefined {
  return pets.get(ctx);
}

/** Creates the local-pet manager of `ctx` (once) and hooks the entity table (a cleared table forgets the pets). */
export function createEggPets(ctx: ClientContext): EggPets {
  const known = pets.get(ctx);
  if (known) return known;

  /** owner object id -> local pet. */
  const byOwner = new Map<number, { id: number; spec: LocalPetSpec }>();
  const byId = new Map<number, number>();
  const waiting = new Map<number, LocalPetSpec>();
  const stats: EggPetStats = { created: 0, replaced: 0, removed: 0, pending: 0 };
  let next = LOCAL_ID_BASE;

  const allocate = (): number | null => {
    for (let tries = 0; tries <= LOCAL_ID_END - LOCAL_ID_BASE; tries += 1) {
      const id = next;
      next = next >= LOCAL_ID_END ? LOCAL_ID_BASE : next + 1;
      if (!byId.has(id) && !ctx.entities.has(id)) return id;
    }
    return null;
  };

  /** Where the new pet stands: at its owner (`pPet->SetPos( GetPos() )`). */
  const ownerPose = (
    owner: EntityRecord,
  ): { pos: [number, number, number]; angle: number } | null => {
    const view = ctx.sim.view;
    const at: [number, number, number] = [0, 0, 0];
    if (view?.valid === true && view.posOf(owner.objId, at)) {
      const a = view.angleOf(owner.objId);
      return { pos: at, angle: Number.isNaN(a) ? 0 : a };
    }
    const spawn = owner.spawn;
    const obj = 'obj' in spawn ? spawn.obj : undefined;
    if (obj?.pos) return { pos: [obj.pos.x, obj.pos.y, obj.pos.z], angle: obj.angleX10 / 10 };
    return null;
  };

  const remove = (ownerId: number): boolean => {
    const cur = byOwner.get(ownerId);
    if (!cur) return false;
    byOwner.delete(ownerId);
    byId.delete(cur.id);
    // the sweep at the end of the drain deletes the record and writes `REMOVE_MOVER` (the world handlers' `onRemove`)
    ctx.entities.markRemoved(cur.id);
    stats.removed += 1;
    return true;
  };

  const build = (ownerId: number, spec: LocalPetSpec): boolean => {
    const owner = ctx.entities.get(ownerId);
    if (!owner || owner.removed || !owner.simHandle) return false;
    const pose = ownerPose(owner);
    if (!pose) return false;
    const id = allocate();
    if (id === null) return false;
    const rt = worldRuntime(ctx);
    // `CreateObj` gives the object the default scale of its model row (a server spawn carries the same number)
    const modelScale = ctx.data?.model(OT_MOVER, spec.moverIndex)?.scale ?? 1;
    const obj = create(ObjCommonSchema, {
      objType: OT_MOVER,
      index: spec.moverIndex,
      scaleX100: modelScale > 0 ? Math.round(modelScale * 100) : 100,
      pos: { x: pose.pos[0], y: pose.pos[1], z: pose.pos[2] },
      angleX10: Math.round(pose.angle * 10),
    });
    const mover = create(MoverCommonSchema, { hp: 1 });
    const npc = create(NpcPartSchema, {});
    const spawn = create(SpawnNpcSchema, { obj, mover, npc });
    const visual = npcVisual(id, obj, mover, npc, ctx.data);
    // the movers row of a pet says AII_PET / AII_EGG; a local pet is a pet whatever the data says
    visual.kind = 'pet';
    if (spec.name !== '') visual.name = spec.name;
    const rec = ctx.entities.spawn(id, {
      kind: EntityKind.LocalPet,
      propId: spec.moverIndex,
      spawn,
      visual,
      hp: 1,
      state: 0,
      petOwner: ownerId,
    });
    const path = skeletonPathOf(ctx, spec.moverIndex);
    const skelId = path === null ? 0 : rt.feeder.skeletonId(path, Prio.Visible);
    ctx.sim.cmd.addMover({
      id,
      kind: KIND.PET,
      isActive: false,
      moverIndex: spec.moverIndex,
      modelType: OT_MOVER,
      modelIndex: spec.moverIndex,
      skelId,
      pos: pose.pos,
      angle: pose.angle,
      state: 0,
      stateFlags: 0,
      motion: 0xffff_ffff,
      scale: rec.visual?.scale ?? 1,
    });
    rec.simHandle = true;
    rt.params.mark(id);
    byOwner.set(ownerId, { id, spec });
    byId.set(id, ownerId);
    stats.created += 1;
    return true;
  };

  const api: EggPets = {
    summon(ownerId, spec) {
      const cur = byOwner.get(ownerId);
      if (cur?.spec.moverIndex === spec.moverIndex) {
        // the same model: only the name (and level) change
        cur.spec = spec;
        const rec = ctx.entities.get(cur.id);
        if (rec?.visual)
          rec.visual.name =
            spec.name !== '' ? spec.name : (ctx.data?.mover(spec.moverIndex)?.name ?? '');
        return;
      }
      if (cur) {
        remove(ownerId);
        stats.replaced += 1;
      }
      if (build(ownerId, spec)) {
        waiting.delete(ownerId);
      } else if (waiting.size < MAX_WAITING || waiting.has(ownerId)) {
        waiting.set(ownerId, spec);
      }
      stats.pending = waiting.size;
    },
    release(ownerId) {
      remove(ownerId);
      waiting.delete(ownerId);
      stats.pending = waiting.size;
    },
    ownerGone(ownerId) {
      // a re-appearing owner brings its pet with the new spawn (`PlayerOther.pet_id`), so nothing is kept
      remove(ownerId);
      waiting.delete(ownerId);
      stats.pending = waiting.size;
    },
    ownerArrived(ownerId) {
      const spec = waiting.get(ownerId);
      if (!spec || byOwner.has(ownerId)) return;
      if (build(ownerId, spec)) waiting.delete(ownerId);
      stats.pending = waiting.size;
    },
    localIdOf: (ownerId) => byOwner.get(ownerId)?.id,
    specOf: (ownerId) => byOwner.get(ownerId)?.spec ?? waiting.get(ownerId),
    isLocal: (objId) => objId >= LOCAL_ID_BASE && objId <= LOCAL_ID_END,
    stats,
    reset() {
      byOwner.clear();
      byId.clear();
      waiting.clear();
      stats.pending = 0;
    },
  };

  // a cleared table (world change, session reset) took the local pets with it; a swept local pet frees its id
  ctx.entities.onRemove((rec, reason) => {
    if (reason === 'clear') {
      api.reset();
      return;
    }
    if (rec.kind === EntityKind.LocalPet) {
      const owner = byId.get(rec.objId);
      if (owner !== undefined) {
        byId.delete(rec.objId);
        if (byOwner.get(owner)?.id === rec.objId) byOwner.delete(owner);
      }
    }
  });
  pets.set(ctx, api);
  return api;
}
