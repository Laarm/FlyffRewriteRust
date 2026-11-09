// `SimCmdWriter` (Plan/40 section 7.4, Plan/31 section 17.4): the encoder of the binary command buffer that
// `SimBridge.frame` hands to `SimWorld.apply_commands`. One method per op; every offset, payload length and parameter
// word comes from the GENERATED `gen/sim/wire.ts` (`OP`, `OP_FIELDS`, `OP_PAYLOAD_LEN`, `PARAMS_LAYOUT`), never from a
// hand-typed number, so a layout change in `flyff_sim::wire` fails the build or the golden test, not a browser session.
// Records are `{op: u16, len: u16 payload bytes, payload}`, little endian, packed; 64-bit frames are two u32 (lo, hi).
// The buffer is a growable `ArrayBuffer` (initial 64 KiB, doubling, never shrinking below 64 KiB).

import {
  MAX_BOUNDS_ENTRIES,
  OP,
  OP_FIELDS,
  OP_PAYLOAD_LEN,
  PARAMS_LAYOUT,
  type OpName,
} from '../gen/sim/wire';
import type { SimMoverParams } from './moverParams';

/** A position in the left-handed game space. */
export type Vec3 = readonly [number, number, number];

/** `DAMAGE_FX.has_tail` bit 0: the legacy `AF_FLYING` tail (`def_pos`, `def_angle`) is valid (`flyff_sim::wire`). */
export const DAMAGE_FX_TAIL = 1;
/** `DAMAGE_FX.has_tail` bit 1: the pad bytes hold the attacker's yaw as 1/65536 of a turn. */
export const DAMAGE_FX_ATTACKER_ANGLE = 2;

/** `flyff_sim::wire::yaw_to_q16`: the `u16` (1/65536 turn) of a yaw in degrees (a truncation like the x86 conversion). */
export function yawToQ16(deg: number): number {
  return Math.trunc(deg * (65536 / 360)) & 0xffff;
}

const INITIAL_BYTES = 64 * 1024;
const HEADER_BYTES = 4;
const TWO32 = 4_294_967_296;

/** Fields of `ADD_MOVER`. */
export interface AddMover {
  readonly id: number;
  /** `wire.KIND`. */
  readonly kind: number;
  readonly isActive: boolean;
  readonly moverIndex: number;
  readonly modelType: number;
  readonly modelIndex: number;
  readonly skelId: number;
  readonly pos: Vec3;
  readonly angle: number;
  readonly angleX?: number;
  readonly state: number;
  readonly stateFlags: number;
  /** `0xFFFFFFFF` = the default motion of the state. */
  readonly motion: number;
  readonly scale: number;
}

/** Fields of `STATE` (`flyff_sim::sync::StateUpdate` plus the stamp). */
export interface StateCmd {
  readonly id: number;
  /** Server frame (`MoverStateNtf.server_frame`). */
  readonly frame: number;
  /** `wire.MOVE_REASON`. */
  readonly reason: number;
  readonly pos: Vec3;
  readonly delta: Vec3;
  readonly angle: number;
  readonly state: number;
  readonly flags: number;
  readonly motion: number;
  readonly motionEx: number;
  readonly loop: number;
  readonly option: number;
  readonly motionFrame: number;
  /** The flying extras, present iff `OBJSTAF_FLY`. */
  readonly fly?: { readonly angleX: number; readonly accPower: number; readonly turnAngle: number };
}

/** The start state of an intent (`IntentStamp`). */
export interface IntentStampCmd {
  readonly pos: Vec3;
  readonly angle: number;
  readonly state: number;
  readonly flags: number;
  readonly angleX: number;
}

/** Fields of `INTENT`; `args` are the six f32 slots of Plan/31 17.4 (target / pattern ids as bit patterns). */
export interface IntentCmd {
  readonly id: number;
  readonly frame: number;
  /** `wire.INTENT`. */
  readonly kind: number;
  readonly stamp: IntentStampCmd;
  readonly args: readonly [number, number, number, number, number, number];
  /**
   * Slots of `args` that carry a `u32` BIT PATTERN instead of a float (object ids, move patterns): `index -> bits`. They are
   * written raw after the floats, so ids above `0x7F800000` (NULL_ID is NaN as a float) survive.
   */
  readonly bitArgs?: Readonly<Record<number, number>>;
}

/** Fields of `SET_SKILL`. */
export interface SetSkillCmd {
  readonly id: number;
  readonly skill: number;
  readonly level: number;
  readonly target: number;
  readonly skillCount: number;
  readonly act: number;
}

/** Fields of `PROJECTILE_SPAWN`. */
export interface ProjectileSpawnCmd {
  readonly projectileId: number;
  readonly kind: number;
  readonly attacker: number;
  readonly target: number;
  readonly frame: number;
  readonly origin: Vec3;
  readonly dest: Vec3;
  readonly speed: number;
  readonly shotIndex: number;
}

/** Fields of `CTRL_ADD`. */
export interface CtrlAddCmd {
  readonly id: number;
  readonly modelKey: number;
  /** 12 floats: three vec4 rows of the LH world matrix (`Mat43`). */
  readonly world: ArrayLike<number>;
  /** `[min x, y, z, max x, y, z]`. */
  readonly aabb: ArrayLike<number>;
  readonly seq: number;
}

/** Fields of `DECOR_ADD`. */
export interface DecorAddCmd {
  readonly id: number;
  readonly kind: number;
  readonly flags: number;
  readonly attach: number;
  readonly modelType: number;
  readonly modelIndex: number;
  readonly clipId: number;
  readonly skelId: number;
  readonly parentMover: number;
  readonly startFrame: number;
  readonly local: ArrayLike<number>;
}

/** The growable command buffer. */
export class SimCmdWriter {
  private buf: ArrayBuffer = new ArrayBuffer(INITIAL_BYTES);
  private view = new DataView(this.buf);
  private bytes = new Uint8Array(this.buf);
  private used = 0;
  private count = 0;
  /** Records written since the last `reset`/`take` (the number `apply_commands` must report back). */
  get records(): number {
    return this.count;
  }

  /** Bytes written. */
  get length(): number {
    return this.used;
  }

  get isEmpty(): boolean {
    return this.used === 0;
  }

  /** Allocated capacity in bytes (probe, tests of the growth rule). */
  get capacity(): number {
    return this.buf.byteLength;
  }

  /** The written prefix. It aliases the writer's buffer: valid until the next write. */
  peek(): Uint8Array {
    return this.bytes.subarray(0, this.used);
  }

  /** Forgets everything written (the buffer is kept). */
  reset(): void {
    this.used = 0;
    this.count = 0;
  }

  /**
   * Drops everything except the (last) `SET_CLOCK` record: `set_world` discards the movers the earlier records
   * addressed, but the clock offset belongs to the session (Plan/40 section 7.3).
   */
  truncateKeepingClock(): void {
    let keep: Uint8Array | null = null;
    let off = 0;
    while (off + HEADER_BYTES <= this.used) {
      const op = this.view.getUint16(off, true);
      const len = this.view.getUint16(off + 2, true);
      if (op === OP.SET_CLOCK) keep = this.bytes.slice(off, off + HEADER_BYTES + len);
      off += HEADER_BYTES + len;
    }
    this.reset();
    if (keep) {
      this.ensure(keep.length);
      this.bytes.set(keep, 0);
      this.used = keep.length;
      this.count = 1;
    }
  }

  /** Writes a `SET_CLOCK` record at the START of the buffer (it must precede the framed commands of this call). */
  prependClock(offset: number): void {
    const size = HEADER_BYTES + OP_PAYLOAD_LEN.SET_CLOCK;
    this.ensure(size);
    this.bytes.copyWithin(size, 0, this.used);
    this.used += size;
    this.count += 1;
    this.i64(this.header('SET_CLOCK', 0), OP_FIELDS.SET_CLOCK.offset.off, offset);
  }

  private ensure(extra: number): void {
    const need = this.used + extra;
    if (need <= this.buf.byteLength) return;
    let size = this.buf.byteLength;
    while (size < need) size *= 2;
    const next = new ArrayBuffer(size);
    new Uint8Array(next).set(this.bytes.subarray(0, this.used));
    this.buf = next;
    this.view = new DataView(next);
    this.bytes = new Uint8Array(next);
  }

  /** Zeroes the record at `at`, writes its header and returns the payload offset (padding stays 0). */
  private header(op: OpName, at: number): number {
    const len = OP_PAYLOAD_LEN[op];
    this.bytes.fill(0, at, at + HEADER_BYTES + len);
    this.view.setUint16(at, OP[op], true);
    this.view.setUint16(at + 2, len, true);
    return at + HEADER_BYTES;
  }

  /** Appends a record and returns its payload offset. */
  private begin(op: OpName): number {
    const total = HEADER_BYTES + OP_PAYLOAD_LEN[op];
    this.ensure(total);
    const p = this.header(op, this.used);
    this.used += total;
    this.count += 1;
    return p;
  }

  private u32(p: number, off: number, v: number): void {
    this.view.setUint32(p + off, v >>> 0, true);
  }

  private i32(p: number, off: number, v: number): void {
    this.view.setInt32(p + off, v | 0, true);
  }

  private u8(p: number, off: number, v: number): void {
    this.view.setUint8(p + off, v & 0xff);
  }

  private f32(p: number, off: number, v: number): void {
    this.view.setFloat32(p + off, v, true);
  }

  /** `u64` from a JS number (exact up to 2^53, the range of `n64`). */
  private u64(p: number, off: number, v: number): void {
    const x = Math.max(0, Math.floor(v));
    this.view.setUint32(p + off, x % TWO32, true);
    this.view.setUint32(p + off + 4, Math.floor(x / TWO32), true);
  }

  private i64(p: number, off: number, v: number): void {
    const x = Math.trunc(v);
    const lo = ((x % TWO32) + TWO32) % TWO32;
    this.view.setUint32(p + off, lo, true);
    this.view.setInt32(p + off + 4, Math.floor(x / TWO32), true);
  }

  private v3(p: number, off: number, v: Vec3): void {
    this.f32(p, off, v[0]);
    this.f32(p, off + 4, v[1]);
    this.f32(p, off + 8, v[2]);
  }

  private f32s(p: number, off: number, v: ArrayLike<number>, n: number): void {
    for (let i = 0; i < n; i += 1) this.f32(p, off + i * 4, v[i] ?? 0);
  }

  // ---- ops ------------------------------------------------------------------------------------------------------

  addMover(m: AddMover): this {
    const F = OP_FIELDS.ADD_MOVER;
    const p = this.begin('ADD_MOVER');
    this.u32(p, F.id.off, m.id);
    this.u8(p, F.kind.off, m.kind);
    this.u8(p, F.is_active.off, m.isActive ? 1 : 0);
    this.u32(p, F.mover_index.off, m.moverIndex);
    this.u32(p, F.model_type.off, m.modelType);
    this.u32(p, F.model_index.off, m.modelIndex);
    this.u32(p, F.skel_id.off, m.skelId);
    this.v3(p, F.pos.off, m.pos);
    this.f32(p, F.angle.off, m.angle);
    this.f32(p, F.angle_x.off, m.angleX ?? 0);
    this.u32(p, F.state.off, m.state);
    this.u32(p, F.state_flags.off, m.stateFlags);
    this.u32(p, F.motion.off, m.motion);
    this.f32(p, F.scale.off, m.scale);
    return this;
  }

  removeMover(id: number): this {
    this.u32(this.begin('REMOVE_MOVER'), OP_FIELDS.REMOVE_MOVER.id.off, id);
    return this;
  }

  /** `SET_PARAMS`: the full parameter block in the order of `PARAMS_LAYOUT` (`bounds` = `FLMB` entry indices). */
  setParams(id: number, params: SimMoverParams): this {
    const entries = params.boundsEntries;
    if (entries.length < 1 || entries.length > MAX_BOUNDS_ENTRIES) {
      throw new RangeError(`bounds_count must be 1..=${String(MAX_BOUNDS_ENTRIES)}`);
    }
    const F = OP_FIELDS.SET_PARAMS;
    const p = this.begin('SET_PARAMS');
    this.u32(p, F.id.off, id);
    const L = PARAMS_LAYOUT;
    const base = p + F.params.off;
    const word = (w: number): number => base + w * 4;
    const putU = (w: number, v: number): void => {
      this.view.setUint32(word(w), v >>> 0, true);
    };
    const putI = (w: number, v: number): void => {
      this.view.setInt32(word(w), v | 0, true);
    };
    const putF = (w: number, v: number): void => {
      this.view.setFloat32(word(w), v, true);
    };
    const putB = (w: number, v: boolean): void => {
      this.view.setUint32(word(w), v ? 1 : 0, true);
    };
    putU(L.kind.word, params.kind);
    putU(L.mover_index.word, params.moverIndex);
    putU(L.ai.word, params.ai);
    putB(L.flying_npc.word, params.flyingNpc);
    putU(L.rank.word, params.rank);
    putB(L.killable.word, params.killable);
    putF(L.base_speed.word, params.baseSpeed);
    putF(L.speed_factor.word, params.speedFactor);
    putI(L.speed_adj_pct.word, params.speedAdjPct);
    putB(L.speed_chg_set.word, params.speedChgSet);
    putI(L.jumping.word, params.jumping);
    putU(L.chr_state.word, params.chrState);
    putU(L.mode.word, params.mode);
    putB(L.base_motion_mode.word, params.baseMotionMode);
    putF(L.attack_speed.word, params.attackSpeed);
    putF(L.casting_ani_speed.word, params.castingAniSpeed);
    putF(L.frame_factor.word, params.frameFactor);
    putB(L.hp_positive.word, params.hpPositive);
    putU(L.level.word, params.level);
    const st = params.stance;
    putB(L.has_stance.word, st !== null);
    if (st) {
      putU(L.stance_kind3.word, st.kind3);
      putU(
        L.stance_flags.word,
        (st.twoHanded ? 1 : 0) | (st.dual ? 2 : 0) | (st.masterModel ? 4 : 0) | (st.psyJob ? 8 : 0),
      );
    }
    const ride = params.ride;
    putB(L.has_ride.word, ride !== null);
    if (ride) {
      putU(L.ride_item.word, ride.item);
      putF(L.ride_flight_speed.word, ride.flightSpeed);
      putF(L.ride_lr_angle.word, ride.lrAngle);
      putF(L.ride_tb_angle.word, ride.tbAngle);
      putU(L.ride_use_motion.word, ride.useMotion);
      putI(L.ride_f_fuel_re_max.word, ride.fuelReMax);
    }
    putU(L.model_ot.word, params.modelOt);
    putU(L.model_index.word, params.modelIndex);
    putU(L.bounds_count.word, entries.length);
    entries.forEach((e, i) => {
      putU(L.bounds_entries.word + i, e);
    });
    putF(L.scale.word, params.scale);
    putI(L.hawkeye_rate.word, params.hawkeyeRate);
    putB(L.disguised.word, params.disguised);
    putU(L.prop_flying.word, params.propFlying);
    putB(L.is_pet.word, params.isPet);
    putU(L.pet_owner.word, params.petOwner);
    return this;
  }

  state(s: StateCmd): this {
    const F = OP_FIELDS.STATE;
    const p = this.begin('STATE');
    this.u32(p, F.id.off, s.id);
    this.u64(p, F.frame.off, s.frame);
    this.u8(p, F.reason.off, s.reason);
    this.u8(p, F.has_fly.off, s.fly ? 1 : 0);
    this.v3(p, F.pos.off, s.pos);
    this.v3(p, F.delta.off, s.delta);
    this.f32(p, F.angle.off, s.angle);
    this.u32(p, F.state.off, s.state);
    this.u32(p, F.flags.off, s.flags);
    this.u32(p, F.motion.off, s.motion);
    this.i32(p, F.motion_ex.off, s.motionEx);
    this.i32(p, F.loop.off, s.loop);
    this.u32(p, F.option.off, s.option);
    this.f32(p, F.motion_frame.off, s.motionFrame);
    this.f32(p, F.angle_x.off, s.fly?.angleX ?? 0);
    this.f32(p, F.acc_power.off, s.fly?.accPower ?? 0);
    this.f32(p, F.turn_angle.off, s.fly?.turnAngle ?? 0);
    return this;
  }

  intent(c: IntentCmd): this {
    const F = OP_FIELDS.INTENT;
    const p = this.begin('INTENT');
    this.u32(p, F.id.off, c.id);
    this.u64(p, F.frame.off, c.frame);
    this.u8(p, F.kind.off, c.kind);
    this.v3(p, F.stamp_pos.off, c.stamp.pos);
    this.f32(p, F.stamp_angle.off, c.stamp.angle);
    this.u32(p, F.stamp_state.off, c.stamp.state);
    this.u32(p, F.stamp_flags.off, c.stamp.flags);
    this.f32(p, F.stamp_angle_x.off, c.stamp.angleX);
    this.f32s(p, F.args.off, c.args, F.args.count);
    if (c.bitArgs) {
      for (const [index, bits] of Object.entries(c.bitArgs)) {
        this.u32(p, F.args.off + Number(index) * 4, bits);
      }
    }
    return this;
  }

  keyframe(id: number, frame: number, pos: Vec3, angle: number, state: number): this {
    const F = OP_FIELDS.KEYFRAME;
    const p = this.begin('KEYFRAME');
    this.u32(p, F.id.off, id);
    this.u64(p, F.frame.off, frame);
    this.v3(p, F.pos.off, pos);
    this.f32(p, F.angle.off, angle);
    this.u32(p, F.state.off, state);
    return this;
  }

  act(
    id: number,
    frame: number,
    msg: number,
    p5: readonly [number, number, number, number, number],
    queueIfBusy: boolean,
  ): this {
    const F = OP_FIELDS.ACT;
    const p = this.begin('ACT');
    this.u32(p, F.id.off, id);
    this.u64(p, F.frame.off, frame);
    this.u32(p, F.msg.off, msg);
    for (let i = 0; i < F.p.count; i += 1) this.i32(p, F.p.off + i * 4, p5[i] ?? 0);
    this.u8(p, F.queue_if_busy.off, queueIfBusy ? 1 : 0);
    return this;
  }

  /**
   * `DAMAGE_FX` (`Damage`): the defender's damage motion and knock-back at `frame`. `tail` is the legacy `AF_FLYING` tail
   * (defender position and yaw); `attackerAngle` is the attacker's yaw exactly as the server used it for the knock-back
   * (`Damage.attacker_angle`, Plan/31 Q10): flag bit 1 and the yaw as a `u16` in 1/65536 turn in the pad bytes
   * (`flyff_sim::wire::yaw_to_q16`, the record size is unchanged); without it the sim uses its copy of the attacker.
   */
  damageFx(
    defender: number,
    frame: number,
    atkFlags: number,
    attacker: number,
    tail: { readonly pos: Vec3; readonly angle: number } | null,
    attackerAngle?: number,
  ): this {
    const F = OP_FIELDS.DAMAGE_FX;
    const p = this.begin('DAMAGE_FX');
    this.u32(p, F.defender.off, defender);
    this.u64(p, F.frame.off, frame);
    this.u32(p, F.atk_flags.off, atkFlags);
    this.u32(p, F.attacker.off, attacker);
    const bits =
      (tail ? DAMAGE_FX_TAIL : 0) | (attackerAngle !== undefined ? DAMAGE_FX_ATTACKER_ANGLE : 0);
    this.u8(p, F.has_tail.off, bits);
    if (attackerAngle !== undefined) {
      this.view.setUint16(p + F.has_tail.off + 1, yawToQ16(attackerAngle), true);
    }
    if (tail) {
      this.v3(p, F.def_pos.off, tail.pos);
      this.f32(p, F.def_angle.off, tail.angle);
    }
    return this;
  }

  pushPower(id: number, frame: number, angle: number, pushAngle: number, power: number): this {
    const F = OP_FIELDS.PUSH_POWER;
    const p = this.begin('PUSH_POWER');
    this.u32(p, F.id.off, id);
    this.u64(p, F.frame.off, frame);
    this.f32(p, F.angle.off, angle);
    this.f32(p, F.push_angle.off, pushAngle);
    this.f32(p, F.power.off, power);
    return this;
  }

  die(id: number, frame: number, causeMsg: number, attacker: number): this {
    const F = OP_FIELDS.DIE;
    const p = this.begin('DIE');
    this.u32(p, F.id.off, id);
    this.u64(p, F.frame.off, frame);
    this.u32(p, F.cause_msg.off, causeMsg);
    this.u32(p, F.attacker.off, attacker);
    return this;
  }

  motionArrive(id: number, objMsg: number): this {
    const F = OP_FIELDS.MOTION_ARRIVE;
    const p = this.begin('MOTION_ARRIVE');
    this.u32(p, F.id.off, id);
    this.u32(p, F.obj_msg.off, objMsg);
    return this;
  }

  setFuel(id: number, fuel: number, accFuel: number): this {
    const F = OP_FIELDS.SET_FUEL;
    const p = this.begin('SET_FUEL');
    this.u32(p, F.id.off, id);
    this.i32(p, F.fuel.off, fuel);
    this.i32(p, F.acc_fuel.off, accFuel);
    return this;
  }

  setActive(id: number): this {
    this.u32(this.begin('SET_ACTIVE'), OP_FIELDS.SET_ACTIVE.id.off, id);
    return this;
  }

  /** `offset` = server frame - local frame (a signed 64-bit value). */
  setClock(offset: number): this {
    this.i64(this.begin('SET_CLOCK'), OP_FIELDS.SET_CLOCK.offset.off, offset);
    return this;
  }

  setVisible(id: number, visible: boolean, wantAttach: boolean): this {
    const F = OP_FIELDS.SET_VISIBLE;
    const p = this.begin('SET_VISIBLE');
    this.u32(p, F.id.off, id);
    this.u8(p, F.visible.off, visible ? 1 : 0);
    this.u8(p, F.want_attach.off, wantAttach ? 1 : 0);
    return this;
  }

  setSkill(c: SetSkillCmd): this {
    const F = OP_FIELDS.SET_SKILL;
    const p = this.begin('SET_SKILL');
    this.u32(p, F.id.off, c.id);
    this.u32(p, F.skill.off, c.skill);
    this.u32(p, F.level.off, c.level);
    this.u32(p, F.target.off, c.target);
    this.i32(p, F.skill_count.off, c.skillCount);
    this.u32(p, F.act.off, c.act);
    return this;
  }

  setSkeleton(id: number, skelId: number, modelType: number, modelIndex: number): this {
    const F = OP_FIELDS.SET_SKELETON;
    const p = this.begin('SET_SKELETON');
    this.u32(p, F.id.off, id);
    this.u32(p, F.skel_id.off, skelId);
    this.u32(p, F.model_type.off, modelType);
    this.u32(p, F.model_index.off, modelIndex);
    return this;
  }

  setTracking(target: number): this {
    this.u32(this.begin('SET_TRACKING'), OP_FIELDS.SET_TRACKING.target.off, target);
    return this;
  }

  setPos(id: number, pos: Vec3): this {
    const F = OP_FIELDS.SET_POS;
    const p = this.begin('SET_POS');
    this.u32(p, F.id.off, id);
    this.v3(p, F.pos.off, pos);
    return this;
  }

  projectileSpawn(c: ProjectileSpawnCmd): this {
    const F = OP_FIELDS.PROJECTILE_SPAWN;
    const p = this.begin('PROJECTILE_SPAWN');
    this.u32(p, F.projectile_id.off, c.projectileId);
    this.u8(p, F.kind.off, c.kind);
    this.u32(p, F.attacker.off, c.attacker);
    this.u32(p, F.target.off, c.target);
    this.u64(p, F.frame.off, c.frame);
    this.v3(p, F.origin.off, c.origin);
    this.v3(p, F.dest.off, c.dest);
    this.f32(p, F.speed.off, c.speed);
    this.u32(p, F.shot_index.off, c.shotIndex);
    return this;
  }

  projectileEnd(projectileId: number, hit: boolean, pos: Vec3): this {
    const F = OP_FIELDS.PROJECTILE_END;
    const p = this.begin('PROJECTILE_END');
    this.u32(p, F.projectile_id.off, projectileId);
    this.u8(p, F.hit.off, hit ? 1 : 0);
    this.v3(p, F.pos.off, pos);
    return this;
  }

  ctrlAdd(c: CtrlAddCmd): this {
    const F = OP_FIELDS.CTRL_ADD;
    const p = this.begin('CTRL_ADD');
    this.u32(p, F.id.off, c.id);
    this.u32(p, F.model_key.off, c.modelKey);
    this.f32s(p, F.world.off, c.world, F.world.count);
    this.f32s(p, F.aabb.off, c.aabb, F.aabb.count);
    this.u32(p, F.seq.off, c.seq);
    return this;
  }

  /**
   * `CTRL_STATE`: ctrl `id` plays from server frame `frame`; `animFrame < 0` plays on from where the ctrl holds. `loop`: `true` = the
   * `LOOP` bit, or the raw `CTRL_STATE_BITS` (`LOOP 1 | HALF 2 | KEEP 4`) of the ctrl's `CtrlProp` kinds.
   */
  ctrlState(id: number, frame: number, animFrame: number, loop: boolean | number): this {
    const F = OP_FIELDS.CTRL_STATE;
    const p = this.begin('CTRL_STATE');
    this.u32(p, F.id.off, id);
    this.u64(p, F.frame.off, frame);
    this.f32(p, F.anim_frame.off, animFrame);
    this.u32(p, F.loop.off, typeof loop === 'number' ? loop & 7 : loop ? 1 : 0);
    return this;
  }

  ctrlRemove(id: number): this {
    this.u32(this.begin('CTRL_REMOVE'), OP_FIELDS.CTRL_REMOVE.id.off, id);
    return this;
  }

  /** `LOCAL_CMD` (43): `range < 0` derives the range in the sim. */
  localCmd(
    act: number,
    target: number,
    range: number,
    ar: number,
    p3: readonly [number, number, number],
    flags: number,
  ): this {
    const F = OP_FIELDS.LOCAL_CMD;
    const p = this.begin('LOCAL_CMD');
    this.u32(p, F.act.off, act);
    this.u32(p, F.target.off, target);
    this.f32(p, F.range.off, range);
    this.u32(p, F.ar.off, ar);
    for (let i = 0; i < F.p.count; i += 1) this.i32(p, F.p.off + i * 4, p3[i] ?? 0);
    this.u8(p, F.flags.off, flags);
    return this;
  }

  localDestPos(pos: Vec3, forward: boolean): this {
    const F = OP_FIELDS.LOCAL_DEST_POS;
    const p = this.begin('LOCAL_DEST_POS');
    this.v3(p, F.pos.off, pos);
    this.u8(p, F.forward.off, forward ? 1 : 0);
    return this;
  }

  localClear(what: number): this {
    this.u8(this.begin('LOCAL_CLEAR'), OP_FIELDS.LOCAL_CLEAR.what.off, what);
    return this;
  }

  localFace(angle: number): this {
    this.f32(this.begin('LOCAL_FACE'), OP_FIELDS.LOCAL_FACE.angle.off, angle);
    return this;
  }

  decorAdd(c: DecorAddCmd): this {
    const F = OP_FIELDS.DECOR_ADD;
    const p = this.begin('DECOR_ADD');
    this.u32(p, F.id.off, c.id);
    this.u8(p, F.kind.off, c.kind);
    this.u8(p, F.flags.off, c.flags);
    this.u8(p, F.attach.off, c.attach);
    this.u32(p, F.model_type.off, c.modelType);
    this.u32(p, F.model_index.off, c.modelIndex);
    this.u32(p, F.clip_id.off, c.clipId);
    this.u32(p, F.skel_id.off, c.skelId);
    this.u32(p, F.parent_mover.off, c.parentMover);
    this.f32(p, F.start_frame.off, c.startFrame);
    this.f32s(p, F.local.off, c.local, F.local.count);
    return this;
  }

  decorRemove(id: number): this {
    this.u32(this.begin('DECOR_REMOVE'), OP_FIELDS.DECOR_REMOVE.id.off, id);
    return this;
  }

  decorVisible(id: number, visible: boolean): this {
    const F = OP_FIELDS.DECOR_VISIBLE;
    const p = this.begin('DECOR_VISIBLE');
    this.u32(p, F.id.off, id);
    this.u8(p, F.visible.off, visible ? 1 : 0);
    return this;
  }
}
