// Movement handlers (Plan/40 section 7.4 message-to-command table, 18 section 5.8, 31 section 15.4; WP-CLIENT-20): the server's
// movement messages become sim commands. The stamps are server frames: a command carries `frame = server_frame` (intents:
// `stamp.server_frame`) and the sim converts it with the clock offset of `SET_CLOCK` (`net/clock.ts` + `frameSkew`,
// CA9), applies the stamped start state at P(F) and fast-forwards to now (cap `fidelity.remote_extrap_cap_frames`, intents 120).
//
//   MoverStateNtf   STATE         relays of players, NPC corrections, server-forced poses
//   MoverKeyframe   KEYFRAME      the 1 Hz drift bound of moving NPCs
//   DestPosNtf      INTENT DEST_POS            SetDestObjNtf  INTENT DEST_OBJ
//   DestAngle       INTENT DEST_ANGLE          SetMovePattern INTENT MOVE_PATTERN      SetSpeedFactor INTENT SPEED_FACTOR
//   SetPos          INTENT SET_POS (others), SET_POS + resetRenderSmoothing (own player, same world; 45 section 6.4)
//   MotionNtf       ACT           (queue_if_busy for remote movers; `obj_msg 0` = a rejected own request, ignored)
//   MotionArrive    MOTION_ARRIVE  PushPower PUSH_POWER
//   SendActMsg      ACT with p4 = 1 (WP-CLIENT-36; AI mode changes: OBJMSG_MODE_PEACE, MODE_TURBO_OFF)
//   Damage          DAMAGE_FX at the server frame, with the AF_FLYING tail and the attacker's exact yaw (WP-CLIENT-36); the HP and
//                   the presentation are `game/combat`
//   ProjectileSpawn PROJECTILE_SPAWN   ProjectileHit PROJECTILE_END (WP-CLIENT-36, the sim part; the visuals are `game/combat`)
//
//   SetFuel         SET_FUEL of the own mover (WP-CLIENT-37: the flying `MoverStateNtf` variants above carry the `fly` tail of
//                   `has_fly = 1`: `angle_x`, `acc_power`, `turn_angle`; `SetMovePattern` is the INTENT row above); the fuel mirror of the
//                   HUD gauge is `game/flight.ts`
//
// The attack notifications and `MoverDeath` are `game/combat` (WP-COMBAT-10). Everything runs in phase Sim; messages about an object
// that is not in the entity table never reach it (H3), except the ones the server addresses to the own session (`obj_id` 0).

import type { ClientContext } from '../../app/context';
import { INTENT } from '../../gen/sim/wire';
import type { IntentStamp } from '../../gen/proto/flyff/v1/common_pb';
import { n64 } from '../../net/int64';
import { Phase } from '../../net/registry';
import { AF, OBJMSG, STAF } from '../combat/constants';
import { flightOf, installFlight } from '../flight';
import type { IntentCmd, IntentStampCmd, Vec3 } from '../simCmd';

const OWNER = 'world.movement';

interface V3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

const v3 = (v: V3 | undefined): Vec3 => [v?.x ?? 0, v?.y ?? 0, v?.z ?? 0];

/** The sim command stamp of an `IntentStamp` (a missing stamp is a start state of zeros at frame 0 = "apply now"). */
export function stampOf(s: IntentStamp | undefined): { frame: number; stamp: IntentStampCmd } {
  return {
    frame: s ? n64(s.serverFrame) : 0,
    stamp: {
      pos: v3(s?.startPos),
      angle: s?.startAngle ?? 0,
      state: s?.startState ?? 0,
      flags: s?.startStateFlags ?? 0,
      angleX: s?.startAngleX ?? 0,
    },
  };
}

/** `INTENT` record of mover `id`. */
function intent(
  kind: number,
  id: number,
  s: IntentStamp | undefined,
  args: readonly [number, number, number, number, number, number],
  bitArgs?: Readonly<Record<number, number>>,
): IntentCmd {
  const { frame, stamp } = stampOf(s);
  return { id, frame, kind, stamp, args, ...(bitArgs ? { bitArgs } : {}) };
}

export function registerMovementHandlers(ctx: ClientContext): void {
  const { registry } = ctx.net;
  const cmd = (): ClientContext['sim']['cmd'] => ctx.sim.cmd;
  const isSelf = (id: number): boolean => ctx.entities.get(id)?.isSelf === true;

  registry.on('moverStateNtf', Phase.Sim, OWNER, (id, m) => {
    cmd().state({
      id,
      frame: n64(m.serverFrame),
      reason: m.reason,
      pos: v3(m.pos),
      delta: v3(m.delta),
      angle: m.angle,
      state: m.state,
      flags: m.stateFlags,
      motion: m.motion,
      motionEx: m.motionEx,
      loop: m.loop,
      option: m.motionOption,
      motionFrame: m.motionFrame,
      ...(m.fly
        ? { fly: { angleX: m.fly.angleX, accPower: m.fly.accPower, turnAngle: m.fly.turnAngle } }
        : {}),
    });
  });

  // `OnSetFuel` (16 section 15): `SetFuel` is addressed to the own session (`obj_id` 0), the command names the active mover; the sim stores
  // the fuel and stops the thrust at 0 (`ACC_STOP`)
  registry.on('setFuel', Phase.Sim, OWNER, (_id, m) => {
    const self = ctx.entities.self();
    if (!self) return;
    cmd().setFuel(self.objId, m.fuel, Math.min(m.accFuel, 0x7fff_ffff));
    flightOf(ctx).onSetFuel(m.fuel, m.accFuel);
  });
  installFlight(ctx);

  // the equipment of a mover changed: the ride rule of `RedoEquip` (the take-off and the landing of the sim copy, `game/flight.ts`)
  registry.on('doEquipNtf', Phase.Sim, OWNER, (objId, _m, mctx) => {
    const id = objId === 0 ? (ctx.entities.self()?.objId ?? 0) : objId;
    flightOf(ctx).rides.request(id, mctx.serverFrame);
  });

  registry.on('moverKeyframe', Phase.Sim, OWNER, (id, m) => {
    cmd().keyframe(id, n64(m.serverFrame), v3(m.pos), m.angle, m.state);
  });

  registry.on('destPosNtf', Phase.Sim, OWNER, (id, m) => {
    const p = v3(m.pos);
    cmd().intent(intent(INTENT.DEST_POS, id, m.stamp, [p[0], p[1], p[2], m.forward ? 1 : 0, 0, 0]));
  });

  registry.on('setDestObjNtf', Phase.Sim, OWNER, (id, m) => {
    // args: target (u32 bits), range
    cmd().intent(
      intent(INTENT.DEST_OBJ, id, m.stamp, [0, m.range, 0, 0, 0, 0], { 0: m.targetObjId }),
    );
  });

  registry.on('destAngle', Phase.Sim, OWNER, (id, m) => {
    cmd().intent(intent(INTENT.DEST_ANGLE, id, m.stamp, [m.angle, m.left ? 1 : 0, 0, 0, 0, 0]));
  });

  registry.on('setMovePattern', Phase.Sim, OWNER, (id, m) => {
    const p = v3(m.pos);
    cmd().intent(
      intent(INTENT.MOVE_PATTERN, id, m.stamp, [0, p[0], p[1], p[2], m.angle, m.angleX], {
        0: m.pattern >>> 0,
      }),
    );
  });

  registry.on('setSpeedFactor', Phase.Sim, OWNER, (id, m) => {
    cmd().intent(intent(INTENT.SPEED_FACTOR, id, m.stamp, [m.factor, 0, 0, 0, 0, 0]));
  });

  registry.on('setPos', Phase.Sim, OWNER, (id, m) => {
    const p = v3(m.pos);
    if (isSelf(id)) {
      // the own player (same world): the sim clears the destination and stops, the smoothing restarts (ref/09a 6.4)
      cmd().setPos(id, p);
      ctx.sim.resetRenderSmoothing();
      return;
    }
    cmd().intent(intent(INTENT.SET_POS, id, m.stamp, [p[0], p[1], p[2], 0, 0, 0]));
  });

  registry.on('motionNtf', Phase.Sim, OWNER, (id, m) => {
    // `obj_msg` 0 is the answer to a rejected own request (legacy AddMotionError)
    if (m.objMsg === 0) return;
    cmd().act(id, n64(m.serverFrame), m.objMsg, [0, 0, 0, 0, 0], !isSelf(id));
  });

  registry.on('motionArrive', Phase.Sim, OWNER, (id, m) => {
    cmd().motionArrive(id, m.objMsg >>> 0);
  });

  registry.on('pushPower', Phase.Sim, OWNER, (id, m) => {
    cmd().pushPower(id, n64(m.serverFrame), m.angle, m.pushAngle, m.power);
  });

  // AI mode changes (`OBJMSG_MODE_PEACE`, `OBJMSG_MODE_TURBO_OFF`): the legacy `OnSendActMsg` passes `(msg, p1, p2, p3, 1, 0)`; the
  // message has no stamp of its own, the batch frame is the moment (31 section 18)
  registry.on('sendActMsg', Phase.Sim, OWNER, (id, m, mctx) => {
    cmd().act(id, mctx.serverFrame, m.objMsg, [m.p1 | 0, m.p2 | 0, m.p3 | 0, 1, 0], false);
  });

  // a resolved hit: the defender's damage motion and knock-back at `server_frame` (the attack hit frame + 1, 11 section 9.4).
  // `AF_FLYING` carries the position and yaw to snap to; the attacker's yaw is the one the server used for its own knock-back
  registry.on('damage', Phase.Sim, OWNER, (id, m, mctx) => {
    const flying = (m.atkFlags & AF.FLYING) !== 0;
    const frame = m.serverFrame !== 0n ? n64(m.serverFrame) : mctx.serverFrame;
    cmd().damageFx(
      id,
      frame,
      m.atkFlags >>> 0,
      m.attacker,
      flying ? { pos: v3(m.defenderPos), angle: m.defenderAngle } : null,
      flying ? m.attackerAngle : undefined,
    );
    // a sitting victim stands up (`OnDamage`: ResetState(OBJSTA_MOVE_ALL)); forced, it is not an input
    const view = ctx.sim.view;
    if (view?.valid === true && (view.stateFlagsOf(id) & STAF.SIT) !== 0) {
      cmd().act(id, frame, OBJMSG.STANDUP, [0, 0, 1, 0, 0], false);
    }
  });

  // server-simulated projectiles (Plan/17 section 4, 31 section 14): the sim runs the same flight from `server_frame`
  registry.on('projectileSpawn', Phase.Sim, OWNER, (attacker, m) => {
    cmd().projectileSpawn({
      projectileId: m.projectileId,
      kind: m.kind,
      attacker,
      target: m.targetObjId,
      frame: n64(m.serverFrame),
      origin: v3(m.origin),
      dest: v3(m.dest),
      speed: m.speed,
      shotIndex: m.shotIndex,
    });
  });
  registry.on('projectileHit', Phase.Sim, OWNER, (_attacker, m) => {
    cmd().projectileEnd(m.projectileId, !m.expired, v3(m.pos));
  });
}
