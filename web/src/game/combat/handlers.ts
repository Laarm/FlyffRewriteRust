// The handlers of the combat messages of milestone M3 (Plan/17 section 11.1, ref/09a section 6.5-6.6; WP-COMBAT-10).
//
//   MeleeAttackNtf, MagicAttackNtf, RangeAttackNtf, SpAttack   Sim   ACT on the attacker's copy (the swing runs in phase)
//   Damage                                                     Core  HP := hp_after, never 0 (death is MoverDeath)
//                                                              Sim   DAMAGE_FX is `game/world/movement.ts` (WP-CLIENT-36)
//                                                              Feature  the display decision tree: numbers, miss/block/parry/
//                                                                       critical effects, the hit shake
//   MoverDeath                                                 Core  HP := 0 (the own character is `dead`), the final hit number
//                                                              Sim   DIE; Feature  focus, next target, the own-death subscribers
//   RevivalNtf, RevivalToLodestarNtf, Resurrection             Core/Sim  leave DEAD
//   SetTargetNtf, ClearTarget                                  Feature  the focus (`ctx.input.targeting`, 43 section 7.3)
//   ProjectileSpawn, ProjectileHit                             Feature  the visuals (`projectiles.ts`); the sim rows are movement.ts
//
// What this module does NOT do, on purpose: the hit sparks and the swing sound of `AttrHit` (the renderer's and the audio
// engine's own sim-event subscriptions; WP-AUDIO-07 plays `meleeHit`), the critical-hit sound (audio trigger T07), the death text
// and the revival window (45, WP-CFEAT-12: they subscribe to `combatRuntime(ctx).onOwnDeath` and read `selfStore.dead`), the
// skill messages (M5, WP-SKILL-12).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { PARTS_RWEAPON } from '../../gen/defs/neuz';
import { StateModeReqSchema } from '../../gen/proto/flyff/v1/character_pb';
import { n64 } from '../../net/int64';
import { EVENT_KINDS, LOCAL_CLEAR } from '../../gen/sim/wire';
import { Phase } from '../../net/registry';
import type { DrainRow } from '../../rules';
import type { SimEventRecord } from '../simEvents';
import { worldRuntime } from '../world/runtime';
import type { EntityRecord } from '../entities';
import { EntityKind } from '../entities';
import { magicAct, meleeAct, rangeAct, specialAct, useMotionOf, isComboMsg } from './attackMap';
import { AF, OBJMSG } from './constants';
import { decideDamageDisplay, SHOW_ALL_RADIUS } from './damageDisplay';
import { DAMAGE_RING, type CombatRuntime } from './runtime';

const OWNER = 'combat';

/** `STATE_BASEMOTION_MODE` and `STATEMODE_BASEMOTION_CANCEL` (`StateModeReq`). */
const STATE_BASEMOTION_MODE = 0x4;
const STATEMODE_BASEMOTION_CANCEL = 2;

/** The row a mover has before the rules answered. */
const NEUTRAL_ROW: DrainRow = {
  speedAdjPct: 0,
  speedChgSet: false,
  chrState: 0,
  hawkeyeRate: 0,
  jumping: 0,
  attackSpeed: 1,
  castingAniSpeed: 1,
};

type V3 = [number, number, number];

export function registerCombatHandlers(ctx: ClientContext, rt: CombatRuntime): void {
  const { registry } = ctx.net;
  const { selfStore } = ctx.stores;
  const world = worldRuntime(ctx);
  const live = world.live;
  const tmp: V3 = [0, 0, 0];

  const isSelf = (id: number): boolean => ctx.entities.get(id)?.isSelf === true;

  /** The logical position of a mover (the render view, else the spawn position). */
  const posOf = (id: number): V3 | null => {
    const view = ctx.sim.view;
    if (view?.valid === true && view.posOf(id, tmp)) return [tmp[0], tmp[1], tmp[2]];
    const spawn = ctx.entities.get(id)?.spawn;
    const p = spawn && 'obj' in spawn ? spawn.obj?.pos : undefined;
    return p ? [p.x, p.y, p.z] : null;
  };

  /** HP of a mover: the own character keeps its live record (published at 10 Hz, ST5), every other mover its entity record. */
  const setHp = (rec: EntityRecord, hp: number): void => {
    rec.hp = hp;
    if (rec.isSelf) {
      live.hp = hp;
      live.dirty = true;
    }
  };

  /** The `ACT` frame of a notification: its own `server_frame`, else the frame of the batch. */
  const frameOf = (stamp: bigint, batchFrame: number): number =>
    stamp !== 0n ? n64(stamp) : batchFrame;

  /** The weapon item of a mover's active hand. */
  const weaponOf = (rec: EntityRecord): number => {
    const w = rec.visual?.equip.get(PARTS_RWEAPON);
    return w ? w.itemId : 0;
  };

  /**
   * The attacker's real animation speed (`ani_speed` of the notification, 03 PL-087): the swing of a remote PLAYER runs at it,
   * so it replaces the attack speed derived from the public data. The parameter record goes into the buffer BEFORE the `ACT`.
   */
  const applyAniSpeed = (id: number, speed: number): void => {
    const rec = ctx.entities.get(id);
    if (rec?.kind !== EntityKind.Player || rec.isSelf) return;
    if (!(speed > 0) || !Number.isFinite(speed)) return;
    const cur = world.params.derivedOf(id) ?? NEUTRAL_ROW;
    if (Math.abs(cur.attackSpeed - speed) < 1e-4) return;
    world.params.setDerived(id, { ...cur, attackSpeed: speed });
    world.params.flushOne(id);
  };

  // ---- the attack notifications: the swing on the attacker's copy -------------------------------------------------------------

  registry.on('meleeAttackNtf', Phase.Sim, OWNER, (id, m, mctx) => {
    rt.stats.attackNtfs += 1;
    if (!isComboMsg(m.atkMsg)) return;
    applyAniSpeed(id, m.aniSpeed);
    ctx.sim.cmd.act(
      id,
      frameOf(m.serverFrame, mctx.serverFrame),
      m.atkMsg,
      meleeAct(m.target, m.param2, m.param3),
      !isSelf(id),
    );
  });

  registry.on('magicAttackNtf', Phase.Sim, OWNER, (id, m, mctx) => {
    rt.stats.attackNtfs += 1;
    if (m.atkMsg !== OBJMSG.ATK_MAGIC1) return;
    applyAniSpeed(id, m.aniSpeed);
    ctx.sim.cmd.act(
      id,
      frameOf(m.serverFrame, mctx.serverFrame),
      OBJMSG.ATK_MAGIC1,
      magicAct(m.target, m.magicPower, m.projectileId),
      !isSelf(id),
    );
  });

  registry.on('rangeAttackNtf', Phase.Sim, OWNER, (id, m, mctx) => {
    rt.stats.attackNtfs += 1;
    if (m.atkMsg !== OBJMSG.ATK_RANGE1) return;
    const rec = ctx.entities.get(id);
    // a player's `item_id` is the charge power; the attack motion is the one of the weapon in its hand. A monster sends the
    // weapon's item id
    const player = rec?.kind === EntityKind.Player;
    const weaponId = player ? weaponOf(rec) : m.itemId;
    const motion = useMotionOf(ctx.data?.item(weaponId), player);
    applyAniSpeed(id, m.aniSpeed);
    ctx.sim.cmd.act(
      id,
      frameOf(m.serverFrame, mctx.serverFrame),
      OBJMSG.ATK_RANGE1,
      rangeAct(m.target, motion, m.projectileId),
      !isSelf(id),
    );
  });

  registry.on('spAttack', Phase.Sim, OWNER, (id, m, mctx) => {
    rt.stats.attackNtfs += 1;
    if (m.atkMsg !== OBJMSG.SP_ATK1 && m.atkMsg !== OBJMSG.SP_ATK2) return;
    const motion = useMotionOf(ctx.data?.item(m.itemId), false);
    ctx.sim.cmd.act(
      id,
      frameOf(m.serverFrame, mctx.serverFrame),
      m.atkMsg,
      specialAct(m.target, m.itemId, motion),
      !isSelf(id),
    );
  });

  // ---- Damage: HP, then the presentation --------------------------------------------------------------------------------------

  registry.on('damage', Phase.Core, OWNER, (victimId, m) => {
    rt.stats.damages += 1;
    const rec = ctx.entities.get(victimId);
    if (!rec) return;
    // `IncHitPoint(-damage)` is the legacy; the rewrite takes the server's value (PL-083). It never reaches 0 here: death is
    // `MoverDeath` (the killing blow sends no `Damage`, Plan/11 G6)
    setHp(rec, m.hpAfter > 0 ? m.hpAfter : 1);
  });

  registry.on('damage', Phase.Feature, OWNER, (victimId, m) => {
    const victim = ctx.entities.get(victimId);
    if (!victim) return;
    const attacker = ctx.entities.get(m.attacker);
    const self = ctx.entities.self();
    const victimIsSelf = victim.isSelf;
    const attackerIsSelf = attacker?.isSelf === true;
    const at = posOf(victimId);
    let near = false;
    if (at && self) {
      const me = posOf(self.objId);
      if (me) near = Math.hypot(at[0] - me[0], at[1] - me[1], at[2] - me[2]) <= SHOW_ALL_RADIUS;
    }
    const hooks = rt.hooks;
    const d = decideDamageDisplay({
      atkFlags: m.atkFlags,
      damage: m.damage,
      victimIsSelf,
      attackerKnown: attacker !== undefined,
      attackerIsSelf,
      showAll: hooks.showAllDamage?.() ?? false,
      victimNear: near,
      authority: hooks.authority?.() ?? selfStore.getState().authority,
      victimIsFocus: ctx.input.targeting?.focus() === victimId,
    });
    const record = {
      victim: victimId,
      attacker: m.attacker,
      damage: m.damage,
      atkFlags: m.atkFlags,
      hpAfter: m.hpAfter,
      shown: d.show,
      effects: d.effects.map((e) => e.xi),
      number: d.number?.value ?? null,
    };
    rt.recent.push(record);
    if (rt.recent.length > DAMAGE_RING) rt.recent.shift();
    if (victimIsSelf) hooks.closeVendorOnHit?.();
    if (!d.show) return;
    rt.stats.damagesShown += 1;
    if ((m.atkFlags & AF.MISS) !== 0) rt.stats.misses += 1;
    if ((m.atkFlags & AF.CRITICAL) !== 0) rt.stats.criticals += 1;
    if ((m.atkFlags & AF.BLOCKING) !== 0) rt.stats.blocks += 1;
    if ((m.atkFlags & AF.PARRY) !== 0) rt.stats.parries += 1;
    if (d.cancelItemCast && hooks.baseMotionActive?.() === true) {
      rt.stats.stateModeCancels += 1;
      if (hooks.cancelBaseMotion) hooks.cancelBaseMotion();
      else {
        ctx.net.sender.send({
          case: 'stateModeReq',
          value: create(StateModeReqSchema, {
            stateMode: STATE_BASEMOTION_MODE,
            flag: STATEMODE_BASEMOTION_CANCEL,
          }),
        });
      }
    }
    const fx = rt.fx();
    if (!fx || !at) return;
    for (const e of d.effects) {
      fx.sfx(e.xi, { pos: [at[0], at[1] + e.lift, at[2]], scale: e.scale, cls: 'hit' });
    }
    if (d.number) {
      rt.stats.numbers += 1;
      fx.damageNumber(at, d.number.value, d.number.style);
      if (d.number.value > 0) fx.hitShake(victimId);
    }
  });

  // ---- MoverDeath -------------------------------------------------------------------------------------------------------------

  /** `(victim, HP before)` of the message being handled (Core writes it, Feature reads it). */
  let dying: { id: number; hpBefore: number } | null = null;

  registry.on('moverDeath', Phase.Core, OWNER, (victimId) => {
    rt.stats.deaths += 1;
    dying = null;
    const rec = ctx.entities.get(victimId);
    if (!rec) return;
    dying = { id: victimId, hpBefore: rec.isSelf ? live.hp : rec.hp };
    setHp(rec, 0);
    world.params.mark(victimId); // `hp_positive` is off now
    if (rec.isSelf) {
      // at once: the revival window opens on this change, the 10 Hz publisher is not waited for
      selfStore.setState({ hp: 0, dead: true });
    }
  });

  registry.on('moverDeath', Phase.Sim, OWNER, (victimId, m, mctx) => {
    ctx.sim.cmd.die(victimId, mctx.serverFrame, m.attackMsg, m.attacker);
  });

  registry.on('moverDeath', Phase.Feature, OWNER, (victimId, m) => {
    const hpBefore = dying !== null && dying.id === victimId ? dying.hpBefore : 0;
    dying = null;
    const victim = ctx.entities.get(victimId);
    if (!victim) return;
    const attacker = ctx.entities.get(m.attacker);
    // the final damage number: the HP the victim had left, when the own character is involved
    if (attacker && (victim.isSelf || attacker.isSelf)) {
      const at = posOf(victimId);
      const fx = rt.fx();
      if (at && fx && hpBefore > 0) {
        rt.stats.numbers += 1;
        fx.damageNumber(at, hpBefore, victim.isSelf ? 0 : 1);
      }
    }
    // focus, next target (`__TARGETSAVE`), the attacker's hit target
    ctx.input.targeting?.onMoverDeath(victimId, m.attacker);
    if (victim.isSelf) {
      rt.stats.ownDeaths += 1;
      rt.emitOwnDeath(m.attacker);
    }
  });

  // ---- revival ----------------------------------------------------------------------------------------------------------------

  /** Core: the mover lives again (its HP follows as `SetPointParam` for the own character). */
  const revive = (id: number): void => {
    const rec = ctx.entities.get(id);
    if (!rec) return;
    if (rec.hp <= 0) setHp(rec, 1);
    world.params.mark(id);
    if (rec.isSelf) selfStore.setState({ dead: false });
  };

  for (const c of ['revivalNtf', 'revivalToLodestarNtf'] as const) {
    registry.on(c, Phase.Core, OWNER, (id) => {
      revive(id);
    });
    registry.on(c, Phase.Sim, OWNER, (id, _m, mctx) => {
      // `ClearState()`: the own mover through the local command, a remote copy through the resurrection action (it clears the
      // state and plays the short get-up motion; the sim has no remote `ClearState`)
      if (isSelf(id)) ctx.sim.cmd.localClear(LOCAL_CLEAR.CASTING);
      else ctx.sim.cmd.act(id, mctx.serverFrame, OBJMSG.RESURRECTION, [0, 0, 0, 0, 0], false);
    });
    registry.on(c, Phase.Feature, OWNER, (id) => {
      rt.stats.revivals += 1;
      if (isSelf(id)) rt.emitOwnRevived();
    });
  }

  registry.on('resurrection', Phase.Core, OWNER, (id) => {
    revive(id);
  });
  registry.on('resurrection', Phase.Sim, OWNER, (id, _m, mctx) => {
    ctx.sim.cmd.act(id, mctx.serverFrame, OBJMSG.RESURRECTION, [0, 0, 0, 0, 0], false);
  });
  registry.on('resurrection', Phase.Feature, OWNER, (id) => {
    rt.stats.revivals += 1;
    if (isSelf(id)) rt.emitOwnRevived();
  });

  // ---- targets ----------------------------------------------------------------------------------------------------------------

  registry.on('setTargetNtf', Phase.Feature, OWNER, (_objId, m) => {
    ctx.input.targeting?.onServerTarget(m.target);
  });
  registry.on('clearTarget', Phase.Feature, OWNER, () => {
    ctx.input.targeting?.onClearTarget();
  });
  for (const c of ['spawnPlayer', 'spawnNpc'] as const) {
    registry.on(c, Phase.Feature, OWNER, (objId) => {
      ctx.input.targeting?.onMoverSpawned(objId);
    });
  }

  // ---- projectiles: the visuals (the sim rows are `game/world/movement.ts`) -------------------------------------------------------

  registry.on('projectileSpawn', Phase.Feature, OWNER, (attacker, m) => {
    rt.stats.projectileSpawns += 1;
    rt.projectiles.spawn(attacker, m);
  });
  registry.on('projectileHit', Phase.Feature, OWNER, (_attacker, m) => {
    if (m.expired) rt.stats.projectileExpired += 1;
    else rt.stats.projectileHits += 1;
    rt.projectiles.end(m);
  });
  // the sim's own contact event (the twin touched the target before the verdict): bookkeeping only
  ctx.loop.onSimEvents((events) => {
    for (const raw of events) {
      const e = raw as SimEventRecord;
      if (e.kind === EVENT_KINDS.ProjectileHit.id) rt.projectiles.contact(e.words[0]);
    }
  });
  registry.on('replace', Phase.Core, OWNER, () => {
    rt.projectiles.clear();
  });
  registry.on('worldReadInfo', Phase.Core, OWNER, () => {
    rt.projectiles.clear();
  });
}
