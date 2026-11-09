// Flight controls (Plan/43 §9; WP-INPUT-15; `WndWorldControlPlayer.cpp:671-952`, `WndWorld.cpp:7357-7543, 7640-7700`): the TS half of
// `ControlFlying`. The act-message sequencing of the broom (thrust, pitch, turns, the turbo and the mouse steering) is the sim's
// (`flyff_sim::control::fly`, Plan/31 §15.2); this module decides WHEN the control runs and feeds it:
//
//   keys          W/S look down/up, A/D turn, Space (rising edge) toggles the acceleration, X (rising edge) the turbo; the record bits
//                 `UP DOWN LEFT RIGHT ACC_TOGGLE TURBO_TOGGLE`
//   steering      the mouse deltas of mode 0 (R held) or mode 1 (free steer, R double click) become the record's `[1]/[2]` +
//                 `MOUSE_MOVED`; Pointer Lock supplies them (D7), `clientX/Y` deltas under pointer capture are the fallback
//   tracking      Z (rising edge) toggles the auto-tracking of the focused mover (`SET_TRACKING`), Tab (rising edge) cycles the fly
//                 targets in ascending ObjId order (PL-165)
//   attack        a held L button or Insert attacks the focus: melee `local_act(ATK1)` (the sim refuses beyond 3 m and reports
//                 `LocalAttack`, which sends the request), a wand `target_check(64 m, +-60 degrees)` then the magic attack
//   hooks         take-off clears the focus, landing leaves mode 1 and the Pointer Lock
//
// The Pointer Lock lives in {@link PointerLockController}: it asks for the lock only on a user gesture of the flight (R down, R double
// click), serialises request and exit, and survives a refusal (the fallback steers without it).

import { IK3_WAND } from '../gen/defs/itemkind';
import { INPUT_BUTTONS } from '../gen/sim/wire';
import { OBJMSG, STA } from '../game/combat/constants';
import type { Vec3LH } from '../render/types';
import type { SelfInfo } from './ports';
import { CONTROL_BLOCKING_APPS, MODE_DONMOVE, REQ_USESKILL, type InputCore } from './worldTick';

/** `cosf(D3DXToRadian(15))`: a mover inside this cone around the camera axis is a fly target (`MoverRender.cpp:871`). */
export const FLY_TARGET_COS = Math.fround(Math.cos((15 * Math.PI) / 180));
/** `cosf(D3DXToRadian(60))`: the flying wand shoots inside this cone (the comment of the original says 30 degrees, the code 60: KEEP). */
export const WAND_CONE_COS = Math.fround(Math.cos((60 * Math.PI) / 180));
/** `IsRangeObj(pObj, 64.0f)`: the flying wand range in metres. */
export const WAND_RANGE = 64;
/** An `Escape` key-down this soon after the Pointer Lock was lost belongs to the browser's lock exit (Plan/43 §2.4, §9.3). */
export const ESC_AFTER_LOCK_MS = 100;
/** `TARGET_CHECK` bits of `target_check` (`flyff_sim::collision::client_query`). */
export const TARGET_IN_RANGE = 1;
export const TARGET_IN_CONE = 2;

// ---- Pointer Lock ---------------------------------------------------------------------------------------------------------------

/** The Pointer Lock operations on the world canvas (the input host wires the DOM; tests pass a fake). */
export interface PointerLockPort {
  /** `canvas.requestPointerLock()`; a refusal arrives later as `PointerLockController.onError`. */
  request(): void;
  /** `document.exitPointerLock()`. */
  exit(): void;
}

/** Counters of the lock (the probe and the Playwright spec read them). */
export interface PointerLockStats {
  /** `requestPointerLock()` calls made. */
  requests: number;
  /** `exitPointerLock()` calls made. */
  exits: number;
  /** `pointerlockerror` events (or an unsupported browser): the fallback steers without the lock. */
  errors: number;
  /** `pointerlockchange` events that reported a lock. */
  acquired: number;
  /** Locks lost while the flight still wanted one (Escape, browser policy, a blur). */
  lost: number;
}

/**
 * The state machine of the lock: `want(true/false)` states what the flight needs, the browser's `pointerlockchange` / `error` events
 * say what happened. Requests and exits are serialised so that the R-down / R-up / R double click of a double click cannot leave a
 * request racing an exit: while a request is pending an exit waits for its answer.
 */
export class PointerLockController {
  /** The flight wants the lock. */
  wanted = false;
  /** A request is out and unanswered. */
  pending = false;
  /** The browser holds the lock. */
  locked = false;
  /** `now` of the last loss of a wanted lock (`-Infinity`: never). */
  lostAt = Number.NEGATIVE_INFINITY;
  readonly stats: PointerLockStats = { requests: 0, exits: 0, errors: 0, acquired: 0, lost: 0 };

  constructor(
    private readonly port: PointerLockPort | undefined,
    /** Called when a wanted lock was lost (the flight leaves mode 1). */
    private readonly onLost: () => void,
  ) {}

  /** The flight needs (`true`) or no longer needs (`false`) the lock. */
  want(on: boolean): void {
    this.wanted = on;
    if (on) {
      if (!this.locked && !this.pending) {
        if (this.port === undefined) {
          this.stats.errors += 1; // no Pointer Lock in this environment: the fallback steers
          return;
        }
        this.pending = true;
        this.stats.requests += 1;
        this.port.request();
      }
    } else if (this.locked) {
      this.stats.exits += 1;
      this.port?.exit();
    }
    // a pending request is answered by the next event, which sees `wanted`
  }

  /** `pointerlockchange`: `locked` is `document.pointerLockElement === canvas`. */
  onChange(locked: boolean, now: number): void {
    this.pending = false;
    this.locked = locked;
    if (locked) {
      this.stats.acquired += 1;
      if (!this.wanted) {
        // the flight let go while the request was in flight
        this.stats.exits += 1;
        this.port?.exit();
      }
      return;
    }
    if (this.wanted) {
      // lost while wanted: Escape, a blur, the browser's policy
      this.wanted = false;
      this.lostAt = now;
      this.stats.lost += 1;
      this.onLost();
    }
  }

  /** `pointerlockerror`: the request was refused (no gesture, the 1 s cool-down after Escape, headless): the fallback applies. */
  onError(): void {
    this.pending = false;
    this.stats.errors += 1;
  }

  /** `true` when the Escape key-down at `now` is the browser's lock exit. */
  escapeIsLockExit(now: number): boolean {
    return now - this.lostAt <= ESC_AFTER_LOCK_MS && now >= this.lostAt;
  }
}

// ---- fly targets ----------------------------------------------------------------------------------------------------------------

/** One candidate of the fly-target list: a visible mover with its position. */
export interface FlyCandidate {
  readonly id: number;
  readonly pos: Vec3LH;
  /** `prop.killable == 1`. */
  readonly killable: boolean;
  /** `prop.flying == 1`. */
  readonly flying: boolean;
}

/**
 * The fly-target list (`MoverRender.cpp:859-877`): visible movers other than the own one that are killable flyers and lie inside the
 * 15 degree cone around the camera axis. The order is ascending ObjId whatever the draw order (the original used the draw order:
 * PL-165), so Tab cycles deterministically.
 */
export function buildFlyTargets(
  candidates: Iterable<FlyCandidate>,
  selfId: number,
  eye: Vec3LH,
  lookAt: Vec3LH,
  out: number[] = [],
): number[] {
  out.length = 0;
  let fx = lookAt[0] - eye[0];
  let fy = lookAt[1] - eye[1];
  let fz = lookAt[2] - eye[2];
  const fl = Math.hypot(fx, fy, fz);
  if (fl < 1e-9) return out;
  fx /= fl;
  fy /= fl;
  fz /= fl;
  for (const c of candidates) {
    if (c.id === selfId || !c.killable || !c.flying) continue;
    const dx = c.pos[0] - eye[0];
    const dy = c.pos[1] - eye[1];
    const dz = c.pos[2] - eye[2];
    const dl = Math.hypot(dx, dy, dz);
    if (dl < 1e-9) continue;
    const dot = (dx * fx + dy * fy + dz * fz) / dl;
    if (dot > FLY_TARGET_COS) out.push(c.id);
  }
  out.sort((a, b) => a - b);
  return out;
}

// ---- the control ------------------------------------------------------------------------------------------------------------------

/** The keys and edges of the last flight control step, as the record needs them. */
export interface FlightFrame {
  up: boolean;
  down: boolean;
  left: boolean;
  right: boolean;
  /** Space rising edge: `ACC_TOGGLE`. */
  acc: boolean;
  /** X rising edge: `TURBO_TOGGLE`. */
  turbo: boolean;
  /** Steering pixels written to the record (`[1]`, `[2]`) and the `MOUSE_MOVED` bit. */
  dx: number;
  dy: number;
}

/** The probe of the flight (`__flyffDebug.input.flight()`). */
export interface FlightSnapshot {
  /** The own mover flew at the last control step (the take-off and landing hooks have run). */
  flying: boolean;
  mouseMode: number;
  tracking: boolean;
  trackId: number;
  select: number;
  targets: number[];
  lock: PointerLockStats & { wanted: boolean; pending: boolean; locked: boolean };
  steerSuspended: boolean;
  frame: FlightFrame;
  attacks: { melee: number; wand: number; refused: number };
  /** The steering pixels written to the records so far (and the records that carried some). */
  steered: { dx: number; dy: number; records: number };
}

const newFrame = (): FlightFrame => ({
  up: false,
  down: false,
  left: false,
  right: false,
  acc: false,
  turbo: false,
  dx: 0,
  dy: 0,
});

export class FlightControl {
  readonly lock: PointerLockController;
  /** `MVRF_TRACKING` of the own mover (the camera's auto-track reads it). */
  tracking = false;
  /** `m_idTracking`: the tracked mover. */
  trackId = 0;
  /** `m_nSelect`: the next index of the Tab cycle. */
  select = 0;
  /** The fly-target list, ascending ObjId (rebuilt every rendered frame while flying). */
  readonly targets: number[] = [];
  /** The last control step. */
  readonly frame: FlightFrame = newFrame();
  /** Mode 0 steering stops after a lost lock until the next R down. */
  steerSuspended = false;
  readonly attacks = { melee: 0, wand: 0, refused: 0 };
  readonly steered = { dx: 0, dy: 0, records: 0 };
  private flew = false;
  /** `s_bAccKeyed`, `s_bTurbo2`, `s_bTraceKeyed`, `s_bSelectKeyed`: the held state of the previous control step. */
  private accKeyed = false;
  private turboKeyed = false;
  private traceKeyed = false;
  private selectKeyed = false;
  /** Steering pixels not yet sent (the remainder of the integer part). */
  private remX = 0;
  private remY = 0;
  private readonly candidates: FlyCandidate[] = [];

  constructor(
    private readonly c: InputCore,
    port: PointerLockPort | undefined,
  ) {
    this.lock = new PointerLockController(port, () => {
      // a wanted lock was lost: mode 1 ends, mode 0 stops steering until the next R down
      if (this.c.mouse.mouseMode === 1) this.c.mouse.mouseMode = 0;
      this.steerSuspended = true;
    });
  }

  // ---- mouse modes (§9.3) ----------------------------------------------------------------------------------------------------------

  /** R down while flying (mode 0 steering): the lock is requested in the gesture. */
  onRButtonDown(): void {
    this.steerSuspended = false;
    if (this.c.mouse.mouseMode === 0) this.lock.want(true);
  }

  /** R up while flying: mode 0 ends its lock; mode 1 keeps it. */
  onRButtonUp(): void {
    if (this.c.mouse.mouseMode === 0) this.lock.want(false);
  }

  /** `m_nMouseMode ^= 1` (R double click while flying): mode 1 takes the lock, leaving it releases the lock. */
  toggleMouseMode(): void {
    const m = this.c.mouse;
    m.mouseMode = m.mouseMode === 0 ? 1 : 0;
    this.steerSuspended = false;
    this.lock.want(m.mouseMode === 1);
  }

  /** `SetMouseMode(0)`: landing, a blur, a world change. */
  leaveMouseMode(): void {
    this.c.mouse.mouseMode = 0;
    this.lock.want(false);
  }

  /** The L-down on empty space while flying resets the acceleration latch (`s_bAccKeyed = FALSE`, `WndWorld.cpp:7202`). */
  accEdgeReset(): void {
    this.accKeyed = false;
  }

  /**
   * `pointerlockchange` / `pointerlockerror` from the DOM. The unlock is handled by the controller (mode 1 ends); a pending
   * `Escape` of the same keystroke is dropped (the lock exit is not an Esc-chain press).
   */
  onLockChange(locked: boolean, now: number): void {
    this.lock.onChange(locked, now);
    if (!locked && this.lock.escapeIsLockExit(now)) this.c.keys.consume('Escape');
  }

  onLockError(): void {
    this.lock.onError();
  }

  /**
   * An `Escape` key-down (the router asks before it records the key). Returns `true` when the flight took it: the lock exit of the
   * browser (a key-down that follows the loss within 100 ms) or the user leaving mode 1 where the browser gave the key to the page.
   * Neither runs the Esc chain (Plan/43 §9.3, WP-INPUT-15).
   */
  onEscapeDown(now: number): boolean {
    if (this.lock.escapeIsLockExit(now)) return true;
    if (this.c.mouse.mouseMode === 1) {
      this.leaveMouseMode();
      return true;
    }
    return false;
  }

  // ---- steering input (called by the pointer move) ----------------------------------------------------------------------------------

  /** Mode 0 with R held, or mode 1: the pointer movement steers. */
  steering(): boolean {
    const m = this.c.mouse;
    return m.mouseMode === 1 || (m.R.down && !this.steerSuspended);
  }

  // ---- take-off and landing (§9.6) --------------------------------------------------------------------------------------------------

  /** Once per world tick: the rising and falling edge of the `FLY` flag. */
  step(self: SelfInfo): void {
    if (self.flying === this.flew) return;
    this.flew = self.flying;
    const c = this.c;
    if (self.flying) {
      // `MODE_FLY`: the old target goes (`SetObjFocus(NULL)`), the auto-attack ends; the action slot is the skill controller's
      c.targeting.setFocus(null);
      c.targeting.autoAttack = false;
      this.accKeyed = c.keys.isHeld('Space');
      this.turboKeyed = c.keys.isHeld('KeyX');
      this.select = 0;
    } else {
      // `MODE_GROUND`: `SetMouseMode(0)` leaves the lock, the camera locks (the camera driver)
      this.leaveMouseMode();
      this.steerSuspended = false;
      this.remX = 0;
      this.remY = 0;
      this.targets.length = 0;
    }
  }

  // ---- the control step (§9.2) --------------------------------------------------------------------------------------------------------

  /**
   * `ControlFlying`: returns `false` when a block holds (the sim gets `BLOCKED`), else records the keys, runs the tracking and attack
   * decisions and returns `true`.
   */
  control(self: SelfInfo): boolean {
    const c = this.c;
    const d = c.deps;
    const f = this.frame;
    f.acc = false;
    f.turbo = false;
    f.dx = 0;
    f.dy = 0;
    if (this.blocked(self)) {
      f.up = f.down = f.left = f.right = false;
      // steering that piled up while the control was blocked must not jump the broom when it runs again
      c.mouse.steerDX = 0;
      c.mouse.steerDY = 0;
      this.remX = 0;
      this.remY = 0;
      return false;
    }
    const k = c.keys;
    // typing in the chat WINDOW never steers the broom (the movement keys count as up, §3.5)
    const typing = d.chat.isFocused();
    const held = (code: string): boolean => !typing && k.isDown(code);
    f.up = held('KeyW');
    f.down = held('KeyS');
    f.left = held('KeyA');
    f.right = held('KeyD');
    // Space toggles the acceleration, X the turbo: rising edges against the previous control step
    const space = !typing && k.isDown('Space');
    f.acc = space && !this.accKeyed;
    this.accKeyed = space;
    const x = !typing && k.isDown('KeyX');
    f.turbo = x && !this.turboKeyed;
    this.turboKeyed = x;
    // Z / Tab (the Z letter action obeys the modifier gate of §3.2)
    const z = !typing && k.isDown('KeyZ') && !k.ctrl && !k.alt && !k.meta;
    if (z && !this.traceKeyed) this.toggleTracking();
    this.traceKeyed = z;
    const tab = !typing && k.isDown('Tab');
    if (tab && !this.selectKeyed) this.cycleTarget();
    this.selectKeyed = tab;
    // the attack of a held L button or Insert
    if (c.mouse.L.down || k.isHeld('Insert')) this.attack(self);
    // the steering pixels of the mouse (the remainder of a fraction stays for the next record)
    this.takeSteering();
    return true;
  }

  /** `ControlPlayer`'s early returns: dead, `DONMOVE_MODE`, a smelting stone, a vendor, the logout, a mini-game, a pending skill answer. */
  private blocked(self: SelfInfo): boolean {
    const c = this.c;
    const w = c.deps.windows;
    return (
      self.dead ||
      (self.modeFlags & MODE_DONMOVE) !== 0 ||
      self.settingStone ||
      self.ownVendorOpen ||
      w.isLogoutPending() ||
      w.anyOpen(CONTROL_BLOCKING_APPS) ||
      (c.reqFlags & REQ_USESKILL) !== 0
    );
  }

  /**
   * `ControlPlayer(WM_LBUTTONDOWN)` of a flying player (the end of `OnLButtonDown`): the attack part of the control runs at the click.
   * While the click holds a mover (`m_bSelectTarget`) the tick does not run the control at all, so this is the swing of the click; the
   * keys and their edges wait for the tick.
   */
  onClick(self: SelfInfo): void {
    if (!self.flying || this.blocked(self)) return;
    if (this.c.mouse.L.down || this.c.keys.isHeld('Insert')) this.attack(self);
  }

  /** Z: the focused mover is tracked, or the tracking ends (`SET_TRACKING`). */
  private toggleTracking(): void {
    const c = this.c;
    const focus = c.targeting.focus;
    const o = focus === null ? null : c.deps.entityInfo(focus);
    if (focus !== null && o?.kind === 'mover') {
      if (this.tracking) this.stopTracking();
      else this.startTracking(focus);
    } else {
      this.stopTracking();
    }
  }

  private startTracking(id: number): void {
    this.tracking = true;
    this.trackId = id;
    this.c.deps.sim.cmd.setTracking(id);
  }

  private stopTracking(): void {
    this.tracking = false;
    this.trackId = 0;
    this.c.deps.sim.cmd.setTracking(0);
  }

  /** Tab: the next fly target becomes the focus; with the tracking on it becomes the tracked mover too. */
  private cycleTarget(): void {
    const list = this.targets;
    if (list.length === 0) return;
    if (this.select >= list.length) this.select = 0;
    const id = list[this.select];
    this.select += 1;
    if (id === undefined) return;
    const c = this.c;
    const o = c.deps.entityInfo(id);
    if (o?.kind !== 'mover') return; // `IsValidObj`
    c.targeting.setFocus(id);
    if (this.tracking) {
      this.trackId = id;
      c.deps.sim.cmd.setTracking(id);
    }
  }

  /** The flying attack of §9.5. */
  private attack(self: SelfInfo): void {
    const c = this.c;
    const focus = c.targeting.focus;
    if (focus === null) return;
    const o = c.deps.entityInfo(focus);
    if (o?.kind !== 'mover') return;
    if (!c.attack.attackable(focus)) {
      this.attacks.refused += 1;
      return;
    }
    // a swing in progress is not asked again: the sim answers it with "ignored" and every ask costs a query slot
    if ((self.state & STA.ATK_ALL) !== 0) return;
    c.deps.hooks?.localAttackStarted?.(); // `PlayCombatMusic` (WndWorldControlPlayer.cpp:814)
    const hand = c.deps.hand?.() ?? null;
    if (hand?.kind3 === IK3_WAND) {
      this.wandAttack(focus);
    } else {
      this.meleeAttack(focus);
    }
  }

  /** `SendActMsg(OBJMSG_ATK1, id)`: the sim refuses beyond 3 m and reports `LocalAttack` (the request leaves from that event). */
  private meleeAttack(target: number): void {
    const sim = this.c.deps.sim;
    if (sim.localAct === undefined) return;
    if ((sim.queryBudget?.() ?? 1) < 1) return; // the next tick asks again (the button is still down)
    const r = sim.localAct(OBJMSG.ATK1, [target | 0, 0, 0, 0, 0]);
    if (r !== undefined && r > 0) this.attacks.melee += 1;
  }

  /**
   * The wand: the target must be inside +-60 degrees of the nose and 64 m (`target_check`), then `DoAttackMagic` (line of sight,
   * `local_act(ATK_MAGIC1)`, `MagicAttackReq`). Both checks are queries; a missing slot is retried by the next tick.
   */
  private wandAttack(target: number): void {
    const sim = this.c.deps.sim;
    if (sim.targetCheck === undefined) return;
    if ((sim.queryBudget?.() ?? 1) < 1) return;
    const bits = sim.targetCheck(target, WAND_RANGE, WAND_CONE_COS);
    if (bits === undefined) return;
    if ((bits & (TARGET_IN_RANGE | TARGET_IN_CONE)) !== (TARGET_IN_RANGE | TARGET_IN_CONE)) return;
    this.attacks.wand += 1;
    this.c.attack.doAttackMagic(target, 0);
  }

  // ---- the record (§6.1) ----------------------------------------------------------------------------------------------------------------

  /** The steering pixels since the last record: whole pixels go out, the fraction stays. */
  private takeSteering(): void {
    const m = this.c.mouse;
    const f = this.frame;
    this.remX += m.steerDX;
    this.remY += m.steerDY;
    m.steerDX = 0;
    m.steerDY = 0;
    f.dx = Math.trunc(this.remX);
    f.dy = Math.trunc(this.remY);
    this.remX -= f.dx;
    this.remY -= f.dy;
    if (f.dx !== 0 || f.dy !== 0) {
      this.steered.dx += f.dx;
      this.steered.dy += f.dy;
      this.steered.records += 1;
    }
  }

  /** The flight bits of the `local_input` word (the keys, the edges and the steering flag). */
  recordBits(): number {
    const f = this.frame;
    let b = 0;
    if (f.up) b |= INPUT_BUTTONS.UP;
    if (f.down) b |= INPUT_BUTTONS.DOWN;
    if (f.left) b |= INPUT_BUTTONS.LEFT;
    if (f.right) b |= INPUT_BUTTONS.RIGHT;
    if (f.acc) b |= INPUT_BUTTONS.ACC_TOGGLE;
    if (f.turbo) b |= INPUT_BUTTONS.TURBO_TOGGLE;
    if (f.dx !== 0 || f.dy !== 0) b |= INPUT_BUTTONS.MOUSE_MOVED;
    return b;
  }

  // ---- the fly-target list (rebuilt every rendered frame) ---------------------------------------------------------------------------

  /** Rebuilds the list from the drawn movers and the displayed camera (only while the own player flies). */
  updateTargets(): void {
    const c = this.c;
    const self = c.deps.self();
    const cam = c.displayed;
    if (self === null || !self.flying || cam === null) {
      this.targets.length = 0;
      return;
    }
    const proxies = c.deps.render()?.visibleObjects?.() ?? [];
    this.candidates.length = 0;
    for (const p of proxies) {
      if (p.kind !== 'mover' || p.id === self.id) continue;
      const o = c.deps.entityInfo(p.id);
      if (o?.pos == null) continue;
      this.candidates.push({
        id: p.id,
        pos: o.pos,
        killable: o.killable === true,
        flying: o.flyingNpc === true,
      });
    }
    buildFlyTargets(this.candidates, self.id, cam.pos, cam.lookAt, this.targets);
  }

  // ---- lifecycle --------------------------------------------------------------------------------------------------------------------------

  /** World entry / exit: the flight state starts over (the sim mover is new). */
  reset(): void {
    this.flew = false;
    this.tracking = false;
    this.trackId = 0;
    this.select = 0;
    this.targets.length = 0;
    this.accKeyed = false;
    this.turboKeyed = false;
    this.traceKeyed = false;
    this.selectKeyed = false;
    this.remX = 0;
    this.remY = 0;
    this.steerSuspended = false;
    Object.assign(this.frame, newFrame());
    this.leaveMouseMode();
  }

  snapshot(): FlightSnapshot {
    return {
      flying: this.flew,
      mouseMode: this.c.mouse.mouseMode,
      tracking: this.tracking,
      trackId: this.trackId,
      select: this.select,
      targets: [...this.targets],
      lock: {
        ...this.lock.stats,
        wanted: this.lock.wanted,
        pending: this.lock.pending,
        locked: this.lock.locked,
      },
      steerSuspended: this.steerSuspended,
      frame: { ...this.frame },
      attacks: { ...this.attacks },
      steered: { ...this.steered },
    };
  }
}
