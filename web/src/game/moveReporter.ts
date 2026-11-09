// MoveReporter (Plan/40 section 7.7, 11 section 9.1, 31 section 15.3; WP-CLIENT-20): turns the report area of the render view into
// the client's movement messages. The loop calls `afterStep(view)` after every iteration that stepped (also in the hidden-tab
// pump, whose `frame()` runs `render_view(1.0)`); an iteration that did not step never re-reads the report (a 144 Hz display
// would send the same report twice).
//
//   1. any of CORR | BEHAVIOR | MOVED | ANGLE | PERIODIC -> ONE `PlayerMoveState`, reason = the highest of
//      CORR > BEHAVIOR > MOVED > ANGLE > PERIODIC, `client_frame = report.client_frame + frameSkew` (CA9), `fly` iff the state
//      flags have FLY, `seq` per session. Coalesced under the key `move`; droppable only for a pure PERIODIC.
//   2. the destination, AFTER the state message of the same report (31 15.3, `__BS_ADJUST_SYNC`): DEST_POS -> `DestPosReq`,
//      DEST_OBJ -> `SetDestObjReq` (an unchanged target is not resent, `__TRAFIC_1222`), KEPT_DEST with CORR -> `DestPosReq` with
//      the kept destination.
//   3. after `SET_ACTIVE` (own spawn, re-attach, world change) one `PlayerMoveState{BEHAVIOR}` goes out with the next report
//      even when no bit is set (ref/09a 6.1: `SendPlayerBehavior` after the own `AddObj`).
//   4. `SendCorrReq` and the other peer-resync sends are not implemented (PL-027).
// The angle field is always the mover's current yaw (fixes the legacy PLAYERCORR2 angle slot, PL-124).

import { create } from '@bufbuild/protobuf';
import { REPORT } from '../gen/sim/wire';
import {
  ClientStampSchema,
  FlyStateSchema,
  MoveReason,
  Vec3Schema,
} from '../gen/proto/flyff/v1/common_pb';
import {
  DestPosReqSchema,
  PlayerMoveStateSchema,
  SetDestObjReqSchema,
} from '../gen/proto/flyff/v1/movement_pb';
import type { ClientStamp } from '../gen/proto/flyff/v1/common_pb';
import type { NetSender } from '../net/sender';
import type { LocalReport } from '../wasm/simTypes';
import { OBJSTAF } from './world/constants';
import type { MoveReporterApi } from './ports';
import type { RenderViewReader } from './renderView';

const BITS = REPORT.bits;
/** `0xFFFFFFFF`: the destination slot holds a position, not an object. */
const NO_OBJECT = 0xffff_ffff;
const STATE_BITS = BITS.CORR | BITS.BEHAVIOR | BITS.MOVED | BITS.ANGLE | BITS.PERIODIC;
const DEST_BITS = BITS.DEST_POS | BITS.DEST_OBJ | BITS.KEPT_DEST;

export interface MoveReporterDeps {
  readonly sender: Pick<NetSender, 'send'>;
  /** `frameSkew = clientFrame - simFrame` of the bridge (CA9). */
  readonly frameSkew: () => number;
  /** `false` while there is no avatar to report for (the report area then holds nothing of ours). */
  readonly active?: () => boolean;
}

export interface MoveReporterStats {
  readonly states: number;
  readonly destPos: number;
  readonly destObj: number;
  readonly lastReason: MoveReason | null;
}

/** The highest reason of the report bits: CORR > BEHAVIOR > MOVED > ANGLE > PERIODIC (31 15.3). */
export function reasonOf(bits: number): MoveReason {
  if ((bits & BITS.CORR) !== 0) return MoveReason.CORR;
  if ((bits & BITS.BEHAVIOR) !== 0) return MoveReason.BEHAVIOR;
  if ((bits & BITS.MOVED) !== 0) return MoveReason.MOVED;
  if ((bits & BITS.ANGLE) !== 0) return MoveReason.ANGLE;
  return MoveReason.PERIODIC;
}

export class MoveReporter implements MoveReporterApi {
  private seq = 0;
  private behaviorArmed = false;
  private lastObj: { target: number; range: number } | null = null;
  private states = 0;
  private destPosSent = 0;
  private destObjSent = 0;
  private last: MoveReason | null = null;

  constructor(private readonly deps: MoveReporterDeps) {}

  /** The next report sends a `BEHAVIOR` state even without a bit (after `SET_ACTIVE`, rule 3). */
  armBehavior(): void {
    this.behaviorArmed = true;
  }

  /** A new session or world: sequence numbers restart, nothing pending, the last destination object is forgotten. */
  reset(): void {
    this.seq = 0;
    this.behaviorArmed = false;
    this.lastObj = null;
  }

  afterStep(view: unknown): void {
    const v = view as RenderViewReader | null;
    if (!v?.valid) return;
    if (this.deps.active && !this.deps.active()) return;
    const bits = v.reportBits;
    const forced = this.behaviorArmed;
    if (bits === 0 && !forced) return;
    const report = v.readReport();
    const skew = this.deps.frameSkew();
    const stateBits = bits & STATE_BITS;
    if (stateBits !== 0 || forced) {
      this.behaviorArmed = false;
      const reason = stateBits === 0 ? MoveReason.BEHAVIOR : reasonOf(stateBits);
      this.sendState(report, reason, skew);
    }
    // a CORR clears the destination on the server: a destination object may be sent again (also the kept one)
    if ((bits & BITS.CORR) !== 0) this.lastObj = null;
    if ((bits & DEST_BITS) !== 0) this.sendDestination(report, bits, skew);
  }

  private sendState(r: LocalReport, reason: MoveReason, skew: number): void {
    const fly = (r.stateFlags & OBJSTAF.FLY) !== 0;
    this.seq += 1;
    // legacy-send: SendPlayerMoved, SendPlayerBehavior, SendPlayerCorr, SendPlayerMoved2, SendPlayerBehavior2, SendPlayerCorr2, SendPlayerAngle
    this.deps.sender.send(
      {
        case: 'playerMoveState',
        value: create(PlayerMoveStateSchema, {
          reason,
          pos: create(Vec3Schema, { x: r.pos[0], y: r.pos[1], z: r.pos[2] }),
          delta: create(Vec3Schema, { x: r.delta[0], y: r.delta[1], z: r.delta[2] }),
          angle: r.angle,
          state: r.state,
          stateFlags: r.stateFlags,
          motion: r.motion,
          motionEx: r.motionEx,
          loop: r.loop,
          motionOption: r.motionOption,
          motionFrame: r.motionFrame,
          clientFrame: (r.clientFrame + skew) >>> 0,
          ...(fly
            ? {
                fly: create(FlyStateSchema, {
                  angleX: r.angleX,
                  accPower: r.accPower,
                  turnAngle: r.turnAngle,
                }),
              }
            : {}),
          seq: this.seq,
        }),
      },
      {
        coalesce: reason === MoveReason.ANGLE ? 'angle' : 'move',
        droppable: reason === MoveReason.PERIODIC,
      },
    );
    this.states += 1;
    this.last = reason;
  }

  private stamp(r: LocalReport, skew: number): ClientStamp {
    return create(ClientStampSchema, {
      clientFrame: (r.clientFrame + skew) >>> 0,
      pos: create(Vec3Schema, { x: r.pos[0], y: r.pos[1], z: r.pos[2] }),
      angle: r.angle,
      state: r.state,
      stateFlags: r.stateFlags,
    });
  }

  private sendDestination(r: LocalReport, bits: number, skew: number): void {
    if (r.destObj !== NO_OBJECT && (bits & (BITS.DEST_OBJ | BITS.KEPT_DEST)) !== 0) {
      const last = this.lastObj;
      if (last?.target === r.destObj && last.range === r.destRange) return; // __TRAFIC_1222
      this.lastObj = { target: r.destObj, range: r.destRange };
      // legacy-send: SendPlayerDestObj
      this.deps.sender.send({
        case: 'setDestObjReq',
        value: create(SetDestObjReqSchema, {
          targetObjId: r.destObj,
          range: r.destRange,
          start: this.stamp(r, skew),
        }),
      });
      this.destObjSent += 1;
      return;
    }
    if ((bits & (BITS.DEST_POS | BITS.KEPT_DEST)) === 0) return;
    this.lastObj = null; // a position destination replaces the followed object
    // legacy-send: SendSnapshot
    this.deps.sender.send({
      case: 'destPosReq',
      value: create(DestPosReqSchema, {
        pos: create(Vec3Schema, { x: r.destPos[0], y: r.destPos[1], z: r.destPos[2] }),
        forward: r.destForward,
        start: this.stamp(r, skew),
      }),
    });
    this.destPosSent += 1;
  }

  get stats(): MoveReporterStats {
    return {
      states: this.states,
      destPos: this.destPosSent,
      destObj: this.destObjSent,
      lastReason: this.last,
    };
  }
}

export function createMoveReporter(deps: MoveReporterDeps): MoveReporter {
  return new MoveReporter(deps);
}
