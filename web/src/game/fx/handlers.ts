// The effect triggers of the network and sim layers (Plan/41 section 17.3, phase `fx`): everything that turns a message or a sim event
// into `RenderFx.sfx` requests and is not a combat decision (those are `game/combat`) nor a skill (M5, WP-SKILL-12).
//
//   CreateSfxObj      `CDPClient::OnCreateSfxObj`: the effect of the server at the anchor object (x = y = z = 0: attached, following it) or at
//                     the absolute position (no follow); an unknown anchor still creates an absolute one
//   RemoveSfxObj      `OnRemoveSfxObj`: the anchor's looping effect ends (`RenderFx.markRemoveSfx`)
//   SetLevel          another player's level-up: `levelUpEffect(job, features.v21_levelup_sfx)` following him (the own character is
//                     told by `SetExperience`: `XI_GEN_LEVEL_UP01`), flight levels the same way (`SetFxp`, `SetFlightLevel`)
//   SpawnNpc          `XI_GEN_MONSTER_SPAWN01` on a monster or NPC that appears outside the join batch within 32 m (XZ) of the own
//                     mover (pets only without the option "hide pets")
//   AttrHit (sim)     the generic hit effects of a melee attack: the weapon's `sfx_obj3` at the victim, `sfx_obj5` at the attacker
//   DustFx, DriftFx   (sim) `XI_NAT_DUST_RUN` at the mover: the skid of a knock-back on the ground (every other tick) and the drift of a
//                     flying turn (`ActionMover.cpp:310`, `ActionMoverState2.cpp:445`); not for an invisible mover (`TRANSPARENT_MODE`)
//
// The sounds of every effect are the audio wrapper's business (`audio/fxAudio.ts`), the damage numbers and the hit shake the combat
// handlers'.

import type { ClientContext } from '../../app/context';
import { AII_PET, PARTS_RWEAPON } from '../../gen/defs/neuz';
import { XI_GEN_LEVEL_UP01, XI_GEN_MONSTER_SPAWN01, XI_NAT_DUST_RUN } from '../../gen/defs/obj';
import { IK3_YOYO } from '../../gen/defs/itemkind';
import { Phase } from '../../net/registry';
import type { SimEventRecord } from '../simEvents';
import { EntityKind } from '../entities';
import { hitSparks } from './hitSpark';
import { levelUpEffect } from './levelUp';
import type { FxRuntime } from './runtime';

const OWNER = 'fx';
/** `TRANSPARENT_MODE` of the mode word. */
const MODE_TRANSPARENT = 0x2;
/** `HitKind` codes of the `AttrHit` payload word (`flyff-sim client/events.rs hit_kind_code`): the melee ones. */
const HIT_KIND_MELEE = 0;
const HIT_KIND_FLY_MELEE = 6;
/** `(vTemp.x^2 + vTemp.z^2) < 32 * 32`. */
const SPAWN_RADIUS_SQ = 32 * 32;

type V3 = [number, number, number];

export function registerFxHandlers(ctx: ClientContext, rt: FxRuntime): void {
  const { registry } = ctx.net;
  const { selfStore } = ctx.stores;
  const tmp: V3 = [0, 0, 0];

  /** The logical position of an object: the render view for movers, the spawn message for the rest. */
  const posOf = (id: number): V3 | null => {
    const view = ctx.sim.view;
    if (view?.valid === true && view.posOf(id, tmp)) return [tmp[0], tmp[1], tmp[2]];
    const spawn = ctx.entities.get(id)?.spawn;
    const p = spawn && 'obj' in spawn ? spawn.obj?.pos : undefined;
    return p ? [p.x, p.y, p.z] : null;
  };
  const selfId = (): number => ctx.entities.self()?.objId ?? 0;

  // ---- CreateSfxObj / RemoveSfxObj ----------------------------------------------------------------------------------------------
  registry.on('createSfxObj', Phase.Fx, OWNER, (objId, m) => {
    const fx = rt.fx();
    if (!fx) {
      rt.stats.noRenderer += 1;
      return;
    }
    rt.stats.createSfxObj += 1;
    const absolute = m.x !== 0 || m.y !== 0 || m.z !== 0;
    const known = objId !== 0 && ctx.entities.has(objId);
    if (known && !absolute) {
      const pos = posOf(objId);
      fx.sfx(m.sfxId, { src: objId, dest: objId, ...(pos ? { pos } : {}) });
    } else if (absolute) {
      fx.sfx(m.sfxId, { pos: [m.x, m.y, m.z] });
    }
  });
  registry.on('removeSfxObj', Phase.Fx, OWNER, (objId, m) => {
    rt.stats.removeSfxObj += 1;
    if (objId !== 0 && ctx.entities.has(objId)) rt.fx()?.markRemoveSfx(objId, m.sfxId);
  });

  // ---- level-ups ----------------------------------------------------------------------------------------------------------------
  // the own character's level before the Core handler applied the message (observers may read stores, not write them)
  let prevLevel = 0;
  let prevFlight = 0;
  registry.on('setExperience', Phase.Pre, OWNER, () => {
    prevLevel = selfStore.getState().level;
  });
  registry.on('setFxp', Phase.Pre, OWNER, () => {
    prevFlight = selfStore.getState().flightLevel;
  });
  const ownLevelUp = (): void => {
    const fx = rt.fx();
    const own = selfId();
    if (!fx || own === 0) return;
    rt.stats.levelUps += 1;
    const pos = posOf(own);
    fx.sfx(XI_GEN_LEVEL_UP01, { src: own, ...(pos ? { pos } : {}) });
  };
  registry.on('setExperience', Phase.Fx, OWNER, (_objId, m) => {
    if (prevLevel > 0 && m.level > prevLevel) ownLevelUp();
    rt.levels.set(selfId(), m.level);
  });
  registry.on('setFxp', Phase.Fx, OWNER, (_objId, m) => {
    if (prevFlight > 0 && m.flightLevel > prevFlight) ownLevelUp();
  });

  const jobOf = (id: number): number => {
    const spawn = ctx.entities.get(id)?.spawn;
    return spawn && 'player' in spawn ? (spawn.player?.job ?? 0) : 0;
  };
  const otherLevelUp = (id: number): void => {
    const fx = rt.fx();
    if (!fx) return;
    rt.stats.levelUps += 1;
    const pos = posOf(id);
    const xi = levelUpEffect(jobOf(id), ctx.features.features.v21_levelup_sfx);
    fx.sfx(xi, { src: id, ...(pos ? { pos } : {}) });
  };
  registry.on('spawnSelf', Phase.Fx, OWNER, (objId, m) => {
    if (m.player) rt.levels.set(objId, m.player.level);
  });
  registry.on('spawnPlayer', Phase.Fx, OWNER, (objId, m) => {
    if (m.player) rt.levels.set(objId, m.player.level);
  });
  registry.on('setLevel', Phase.Fx, OWNER, (objId, m) => {
    const id = objId === 0 ? selfId() : objId;
    const prev = rt.levels.get(id);
    rt.levels.set(id, m.level);
    // the server tells the others only; an own copy is a no-op here (`SetExperience` shows the own effect)
    if (id === selfId()) return;
    if (prev === undefined || m.level > prev) otherLevelUp(id);
  });
  registry.on('setFlightLevel', Phase.Fx, OWNER, (objId) => {
    if (objId !== 0 && objId !== selfId()) otherLevelUp(objId);
  });
  registry.on('delObj', Phase.Fx, OWNER, (objId) => {
    rt.levels.delete(objId);
  });

  // ---- ground items ---------------------------------------------------------------------------------------------------------------
  // a drop bounces (Plan/41 11.9); the items of the join batch lie where they are
  registry.on('spawnItem', Phase.Fx, OWNER, (objId, _m, mctx) => {
    if (mctx.inJoin) return;
    rt.fx()?.itemDropped?.(objId);
  });

  // ---- ctrls ------------------------------------------------------------------------------------------------------------------
  // `CommonPlace` (Plan/41 11.8, 17.2): the effect of the ctrl on ACTION and the fade on ALPHA; the animation is the sim's (`game/world`)
  registry.on('commonPlace', Phase.Fx, OWNER, (objId, m) => {
    rt.fx()?.ctrlPlace?.(objId, m.kind);
  });

  // ---- monster spawn ------------------------------------------------------------------------------------------------------------
  registry.on('spawnNpc', Phase.Fx, OWNER, (objId, m, mctx) => {
    if (mctx.inJoin) return;
    const fx = rt.fx();
    const own = selfId();
    const at = m.obj?.pos;
    if (!fx || own === 0 || !at) return;
    const me = posOf(own);
    if (!me) return;
    const dx = at.x - me[0];
    const dz = at.z - me[2];
    if (dx * dx + dz * dz >= SPAWN_RADIUS_SQ) return;
    if (rt.hooks.hidePets?.() === true && ctx.data?.mover(m.obj?.index ?? 0)?.ai === AII_PET)
      return;
    rt.stats.spawnEffects += 1;
    fx.sfx(XI_GEN_MONSTER_SPAWN01, { src: objId, pos: [at.x, at.y, at.z] });
  });

  // ---- the generic hit effects (sim event AttrHit) --------------------------------------------------------------------------------
  const onHit = (e: SimEventRecord): void => {
    const kind = e.words[0];
    if (kind !== HIT_KIND_MELEE && kind !== HIT_KIND_FLY_MELEE) return;
    const fx = rt.fx();
    if (!fx) return;
    const attacker = ctx.entities.get(e.objId);
    if (attacker?.kind !== EntityKind.Player) return;
    const equipped = attacker.visual?.equip.get(PARTS_RWEAPON);
    // the sparks of the particle manager (the hit pause of a player's melee swing): the upgrade level of the weapon
    fx.meleeSparks?.(e.objId, (equipped?.option ?? 0) & 0xff);
    const weapon = equipped?.itemId;
    const prop = weapon === undefined ? undefined : ctx.data?.item(weapon);
    if (!prop) return;
    const target = e.words[2];
    const victim = posOf(target);
    const src = posOf(e.objId);
    const view = ctx.sim.view;
    if (!victim || !src || view?.valid !== true) return;
    const sparks = hitSparks(
      prop.sfx_obj[2],
      prop.sfx_obj[4],
      victim,
      src,
      view.angleOf(e.objId),
      prop.item_kind3 === IK3_YOYO,
    );
    for (const s of sparks) {
      rt.stats.hitSparks += 1;
      fx.sfx(s.xi, { pos: [s.pos[0], s.pos[1], s.pos[2]], cls: 'hit' });
    }
  };
  ctx.sim.events.on(['AttrHit'], onHit);

  // ---- the dust of the skids and drifts (sim events DustFx / DriftFx) -------------------------------------------------------------
  ctx.sim.events.on(['DustFx', 'DriftFx'], (e: SimEventRecord) => {
    const fx = rt.fx();
    if (!fx) return;
    const rec = ctx.entities.get(e.objId);
    if (((rec?.visual?.mode ?? 0) & MODE_TRANSPARENT) !== 0) return;
    const p = posOf(e.objId);
    if (!p) return;
    rt.stats.dust += 1;
    fx.sfx(XI_NAT_DUST_RUN, { pos: p });
  });
}
