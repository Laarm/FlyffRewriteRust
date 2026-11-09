// The per-tick world input (Plan/43 §2.1-§2.3, §3.4-§3.7, §6.1-§6.8; WP-INPUT-05, WP-INPUT-08). `WorldTick.tick(nowMs)` runs
// once per iteration that steps at least one 60 Hz frame, in the order of the original tick (ref/21 §5.3 step 7):
//
//   1. manager step   (`CWndMgr::Process`)      the Esc chain and the taskbar page keys 1-8
//   2. taskbar step   (`CWndTaskBar::Process`)  Enter -> chat, applet hotkeys, F1-F9 item slots (equip latch)
//   3. world step     (`CWndWorld::Process`)    close-on-move, the control decision (§6.2), the local input record
//
// and ends with `KeyState.endTick()`. The returned record is the `local_input` word array of Plan/31 §17.5: held button bits
// apply to every stepped frame of the iteration, edge bits to the first.
//
// The attack branch of the control decision (a held click on a target, auto-attack) is `core.attack` (WP-INPUT-10); the bow and
// wand charge is evaluated after it in the same tick (WP-INPUT-11, PL-121). Reserved skills are WP-INPUT-13/14.

import {
  APP_COMMUNICATION_CHAT,
  APP_INVENTORY,
  APP_MINIGAME_DICE,
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_PUZZLE,
  APP_MINIGAME_WORD,
} from '../gen/defs/resdata';
import { INPUT_BUTTONS, INPUT_SLOT } from '../gen/sim/wire';
import { hotkeyAllowed } from '../ui/wm/keyContract';
import type { AutoRun } from './autorun';
import { escChain } from './escChain';
import { appletHotkeyActive, APPLET_HOTKEYS, KeyAction, codesOf, modifierGateOpen } from './keymap';
import type { KeyState, Modifiers } from './keys';
import { LR_JUMP_MS, type WorldMouse } from './pointer';
import { CreateMonsterPlacement } from './placement';
import type { InputDeps, SelfInfo } from './ports';
import { FOCUS_ATTACK_MS, NEXTSKILL_ACTIONSLOT, NEXTSKILL_NONE, type Targeting } from './targeting';
import type { AttackController } from './attack';
import type { FlightControl } from './flight';
import type { SkillController } from './skill';
import type { ChargeGauge, GaugeDisplay } from './charge';
import { autoShot } from './charge';
import { IK3_BOW, IK3_WAND } from '../gen/defs/itemkind';
import type { LocalCommands } from './commands';
import type { CameraDriver } from './camera/cameraControls';
import { groundPick, pickRayLH, type GroundPick } from './picking';
import type { GuildWarTab, GwCandidate, GwTabEnv } from './guildWarTab';
// ---- housing / guild house (ui-house-quiz)
import { DeployRouting } from './deploy';

/** `DONMOVE_MODE` (`authorization.h:23`): the client blocks movement. */
export const MODE_DONMOVE = 0x8;
/** `OBJSTA_STAND` (bit 0), `OBJSTA_MOVE_ALL` and `OBJSTA_BMOVE`: the close-on-move rule reads the state word. */
export const OBJSTA_STAND = 1;
export const OBJSTA_MOVE_ALL = 0xff;
export const OBJSTA_BMOVE = 5;
/** `REQ_USESKILL`: a `UseSkillReq` awaits its answer (WP-INPUT-13 sets it). */
export const REQ_USESKILL = 1;
/** `OBJACT_USESKILL`. */
const OBJACT_USESKILL = 5;
/** The follow upkeep re-issues the destination beyond 4 m (`|d|^2 > 16`). */
export const FOLLOW_REDEST_DIST_SQ = 16;
/** Windows whose presence stops the ground and the flight control (`__EVE_MINIGAME`). */
export const CONTROL_BLOCKING_APPS: readonly number[] = [
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_DICE,
  APP_MINIGAME_WORD,
  APP_MINIGAME_PUZZLE,
];

/** The shared state of the input layer (every module reads it, `createInput` builds it). */
export interface InputCore {
  readonly deps: InputDeps;
  readonly keys: KeyState;
  readonly mouse: WorldMouse;
  readonly targeting: Targeting;
  readonly commands: LocalCommands;
  readonly autorun: AutoRun;
  readonly camera: CameraDriver;
  /** The last displayed camera (the pick ray source), `null` before the first frame. */
  displayed: {
    pos: readonly [number, number, number];
    lookAt: readonly [number, number, number];
    up: readonly [number, number, number];
    fovDiv: number;
  } | null;
  /** `m_objidTracking`: the ground follow target. */
  followId: number | null;
  /** The attacks of the own character (WP-INPUT-10): `useFocusObj`, the arrival of a command, the combo requests. */
  readonly attack: AttackController;
  /** The bow / wand charge gauge (WP-INPUT-11). */
  readonly charge: ChargeGauge;
  /** `reqFlags & REQ_USESKILL`. */
  reqFlags: number;
  /** The flight controls: steering, Pointer Lock, fly targets, tracking and the flying attack (WP-INPUT-15). */
  readonly flight: FlightControl;
  /** Skills from the taskbar and the action slot (WP-INPUT-13, -14). */
  readonly skill: SkillController;
  /** The Tab key on the siege map: the list of enemy players and the session-long cycle counter (WP-INPUT-16). */
  readonly gwTab: GuildWarTab;
  /** The ground point under the cursor (`undefined`: no query slot this iteration); set by `createInput`. */
  pickGround?(): GroundPick | null | undefined;
  /** The system line of a client text id (`TID_GAME_BLOCKTARGETING`, ...). */
  notify(tid: number): void;
}

/** What `groundControl` decided for the sim record of this tick. */
interface Frame {
  up: boolean;
  down: boolean;
  left: boolean;
  right: boolean;
  space: boolean;
}

export class WorldTick {
  private readonly rec = new Int32Array(5);
  private readonly equipLatch: boolean[] = new Array<boolean>(9).fill(false);
  private walkLatch = false;
  /** The own mover flew in the previous tick (the take-off edge cancels the action slot). */
  private wasFlying = false;
  /** A click destination whose ground pick found no query slot: retried by the next tick. */
  private clickPending = false;
  /** A create-monster click whose ground pick or `height_attr` found no query slot: retried by the next tick while the button is down. */
  private placementPending = false;
  /** The time of an L+R chord press that no decision tick has consumed yet (`noteChord`); owner #002. */
  private chordAt = Number.NEGATIVE_INFINITY;
  /** This tick's ground record carries JUMP because of the chord (set by the decision, read by `buildRecord`). */
  private chordJump = false;
  /** World entry: the next tick gives the world the keyboard focus (see `tick`). */
  private focusWorldPending = false;
  private frame: Frame = { up: false, down: false, left: false, right: false, space: false };
  /** The control decision of the last tick (the probe and the tests read it). */
  controlRuns = false;
  /** The charge gauge value of the last tick (`RenderGauge(0..4)`, `null` = no gauge). */
  gauge: GaugeDisplay = null;
  /** The `buttons` word of the last record (the probe reads it). */
  lastButtons = 0;
  /** JUMPs sent because of an L+R chord (one per chord; the probe reads it). */
  chordJumps = 0;
  lastEsc: string | null = null;
  ticks = 0;
  /** Click destinations written (one per click) and the last ground pick, for the debug surface. */
  destinations = 0;
  lastGround: GroundPick | null = null;
  /** The last ground-pick ray (LH), for the debug surface. */
  lastRay: { origin: readonly number[]; dir: readonly number[] } | null = null;

  /** The create-monster placement mode (step 5 of the ground control). */
  readonly placement: CreateMonsterPlacement;
  // ---- housing / guild house (ui-house-quiz)
  /** The routing of the furniture placement (`input/deploy.ts`, WP-INPUT-17). */
  readonly deploy: DeployRouting;

  constructor(private readonly c: InputCore) {
    this.placement = new CreateMonsterPlacement(c);
    this.deploy = new DeployRouting(c);
  }

  // ---- the tick ------------------------------------------------------------------------------------------------------

  /** One world tick; `null` when there is no world to control (no record is sent). */
  tick(now: number): Int32Array | null {
    const c = this.c;
    const d = c.deps;
    if (d.mode() !== 'game') {
      c.keys.endTick();
      return null;
    }
    const self = d.self();
    if (self === null) {
      c.keys.endTick();
      return null;
    }
    this.ticks += 1;
    if (this.focusWorldPending) {
      // `OpenField` ends with `ObjectExecutor(APP_WORLD)`: the world, not the chat window, has the focus after the entry
      // (the HUD module opens the windows when the screen changes, so this is the first tick AFTER them)
      this.focusWorldPending = false;
      c.deps.windows.focusWorld?.();
    }
    this.managerStep(self);
    this.taskbarStep();
    this.worldStep(self, now);
    c.keys.endTick();
    return this.buildRecord(self);
  }

  // ---- 1. CWndMgr::Process (§3.4, §3.6) --------------------------------------------------------------------------------

  private restrictions(): { draggingShortcut: boolean; vendorWindowOpen: boolean } {
    const w = this.c.deps.windows;
    return { draggingShortcut: w.isDraggingShortcut(), vendorWindowOpen: w.isVendorWindowOpen() };
  }

  private managerStep(self: SelfInfo): void {
    const c = this.c;
    const w = c.deps.windows;
    if (w.isLogoutPending()) return;
    const r = this.restrictions();
    if (r.vendorWindowOpen) return; // `CWndMgr::Process` returns early: no Esc chain, no page keys
    if (c.keys.isPending('Escape')) {
      c.keys.consume('Escape');
      if (hotkeyAllowed('Escape', r)) {
        this.lastEsc = escChain({
          windows: w,
          hooks: c.deps.hooks ?? {},
          targeting: c.targeting,
          commands: c.commands,
          dead: () => self.dead,
          tracking: () => false,
          hostileMover: (id) => {
            const o = c.deps.entityInfo(id);
            return o !== null && o.kind === 'mover' && !o.peaceful;
          },
        });
      }
    }
    for (const k of codesOf(KeyAction.ItemPage)) {
      if (!c.keys.consume(k)) continue;
      const n = Number(k.slice('Digit'.length)) - 1;
      if (hotkeyAllowed(k, r) && n >= 0 && n < 8) c.deps.taskbar.setItemPage(n);
    }
  }

  // ---- 2. CWndTaskBar::Process (§3.3, §3.4, §3.7) ----------------------------------------------------------------------

  private taskbarStep(): void {
    const c = this.c;
    const w = c.deps.windows;
    if (w.isLogoutPending()) return;
    const r = this.restrictions();
    // Enter: open or focus the chat (`WndTaskBar.cpp:2378`)
    for (const k of ['Enter', 'NumpadEnter']) {
      if (c.keys.consume(k) && hotkeyAllowed(k, r)) c.deps.chat.focusInput();
    }
    // applet hotkeys; the modifier gate was applied at the key-down, the feature gate is read now
    const features = c.deps.features();
    for (const h of APPLET_HOTKEYS) {
      if (!c.keys.isPending(h.code)) continue;
      if (!appletHotkeyActive(h, features)) continue;
      if (h.code === 'KeyR' && c.deps.chat.isOpen()) continue; // the whisper reply owns R (PL-141): see `groundControl`
      c.keys.consume(h.code);
      if (hotkeyAllowed(h.code, r)) w.objectExecutorApplet(h.app);
    }
    // F1-F9 item slots with the equip latch (`WndTaskBar.cpp:2430-2465`)
    for (let i = 0; i < 9; i += 1) {
      const code = `F${String(i + 1)}`;
      if (!c.keys.consume(code)) continue;
      if (!hotkeyAllowed(code, r)) continue;
      if (this.equipLatch[i] === true) continue; // an equip item executes once per press, not per auto-repeat
      if (c.deps.taskbar.slotIsEquipItem(i)) this.equipLatch[i] = true;
      c.deps.taskbar.executeItemSlot(i);
    }
  }

  // ---- 3. CWndWorld::Process (§6) --------------------------------------------------------------------------------------

  private worldStep(self: SelfInfo, now: number): void {
    const c = this.c;
    // §6.8: the mover is not standing still (`!IsState(OBJSTA_STAND)` is a BIT test, so STAND2 counts as moving, KEEP;
    // walking backwards has bit 0 set and is named explicitly): a dragged shortcut is dropped, windows close
    if (
      self.state !== 0 &&
      ((self.state & OBJSTA_STAND) === 0 || (self.state & OBJSTA_MOVE_ALL) === OBJSTA_BMOVE)
    ) {
      c.deps.windows.cancelDragUnlessFrom(APP_INVENTORY);
      c.deps.hooks?.closeOnMove?.();
    }
    if (this.clickPending && c.mouse.L.down && c.mouse.freeMove) this.clickDestination();
    // ---- housing / guild house (ui-house-quiz): the terrain point under the pointer drives the furniture ghost
    this.deploy.tick();
    // a placement click whose queries found no slot: the held button is tried again until it is served (the ground control skips a held click)
    if (this.placementPending) {
      if (c.mouse.L.down) {
        this.placementPending = this.placement.step(self, false) === 'retry';
      } else {
        this.placementPending = false;
      }
    }
    // take-off cancels the action slot (Plan/43 §9.6); the queue target is watched every tick (`CWndTaskBar::Process`)
    if (self.flying && !this.wasFlying) c.skill.onTakeOff();
    this.wasFlying = self.flying;
    c.skill.checkQueueTarget();
    c.flight.step(self);
    // the L+R chord latched by the press handlers is consumed by the FIRST decision tick after it (flight and a blocked control drop it)
    const chord = now - this.chordAt < LR_JUMP_MS;
    this.chordAt = Number.NEGATIVE_INFINITY;
    this.chordJump = false;
    this.controlRuns = self.flying
      ? this.flyingDecision(self)
      : this.groundDecision(self, now, chord);
    c.mouse.L.downed = c.mouse.L.down;
    this.chargeStep(self, now);
  }

  /**
   * The bow / wand charge (§7.7, PL-121): with a live mover in focus the gauge shows; a held click on it that reaches the full
   * charge (a wand with less than 5 MP: level 3) fires by itself, the power as computed, and locks the gauge for 1800 ms.
   */
  private chargeStep(self: SelfInfo, now: number): void {
    const c = this.c;
    const t = c.targeting;
    const focus = t.focus;
    const o = focus === null ? null : c.deps.entityInfo(focus);
    const live = focus !== null && o !== null && o.kind === 'mover' && !o.dead && !self.dead;
    if (live) {
      const hand = c.deps.hand?.() ?? null;
      const kind = hand?.kind3 === IK3_BOW ? 'bow' : hand?.kind3 === IK3_WAND ? 'wand' : 'other';
      const power = c.charge.power(now);
      const shoot = autoShot(kind, power, self.mp ?? 0, c.mouse.L.down, t.selectTarget);
      if (shoot !== null && c.attack.attackable(focus)) {
        c.charge.lock(now);
        if (shoot === 'wand') c.commands.magicAttack(focus, power);
        else c.commands.rangeAttack(focus, power);
      }
    }
    this.gauge = c.charge.display(now, live, c.mouse.L.down, t.selectTarget);
  }

  /**
   * The ground branch of §6.2. `chord`: an L+R chord was pressed since the last decision (owner #002): the control runs once even
   * though a button is held, and the record carries JUMP when it ran (a blocked control drops the chord).
   */
  private groundDecision(self: SelfInfo, now: number, chord: boolean): boolean {
    const c = this.c;
    const t = c.targeting;
    const m = c.mouse;
    let runs: boolean;
    if (t.nextSkill === NEXTSKILL_ACTIONSLOT) {
      // `useSkillToFocus` (WP-INPUT-14): a refusal keeps the reservation, the next tick tries again
      c.skill.useSkillToFocus(t.focus);
      runs = false;
    } else if (t.nextSkill !== NEXTSKILL_NONE) {
      // `useSkillShortcut` (WP-INPUT-13): the reserved skill of a slot
      c.skill.useSkillShortcut(t.nextSkill);
      runs = false;
    } else if ((m.L.down || t.autoAttack) && c.commands.localCmd !== OBJACT_USESKILL) {
      if (chord) {
        runs = this.groundControl(self, now);
        this.chordJump = runs;
      } else if (t.autoAttack || (t.selectTarget && now - t.tFocusAttack >= FOCUS_ATTACK_MS)) {
        const obj = t.focus;
        if (obj !== null) {
          runs = false;
          // a mover, a ground item or a ctrl; `attackable` ends the auto-attack of a target that cannot be attacked
          if (
            c.deps.entityInfo(obj) !== null &&
            c.attack.attackable(obj) &&
            (c.reqFlags & REQ_USESKILL) === 0
          ) {
            c.attack.useFocusObj(obj);
          }
        } else {
          runs = this.groundControl(self, now);
        }
      } else {
        runs = false; // L held on a fresh target (< 500 ms) or on the ground: no control
      }
    } else {
      runs = this.groundControl(self, now);
      this.chordJump = chord && runs; // a chord whose buttons were both released before the tick
      t.selectTarget = false;
    }
    return runs;
  }

  /**
   * The flying branch of §6.2 (`WndWorld.cpp:8750-8770`): a held click on a ground item or a ctrl walks there once; otherwise the
   * flight control runs.
   */
  private flyingDecision(self: SelfInfo): boolean {
    const c = this.c;
    const t = c.targeting;
    const m = c.mouse;
    if (m.L.down && t.selectTarget) {
      const obj = t.focus;
      const o = obj === null ? null : c.deps.entityInfo(obj);
      if (obj !== null && (o?.kind === 'item' || o?.kind === 'ctrl') && !m.L.downed) {
        c.commands.useItem(obj);
      }
      return false;
    }
    const runs = c.flight.control(self);
    t.selectTarget = false;
    return runs;
  }

  /**
   * `groundControl()` (§6.2): the TS part of `ControlGround`. Returns `false` when a block holds (the sim gets `BLOCKED`),
   * else runs steps 1-8 and returns `true`.
   */
  groundControl(self: SelfInfo, now: number): boolean {
    const c = this.c;
    const d = c.deps;
    const w = d.windows;
    if (
      self.dead ||
      (self.modeFlags & MODE_DONMOVE) !== 0 ||
      self.settingStone ||
      self.ownVendorOpen ||
      w.isLogoutPending() ||
      w.anyOpen(CONTROL_BLOCKING_APPS) ||
      (c.reqFlags & REQ_USESKILL) !== 0
    ) {
      return false;
    }
    const k = c.keys;
    // 1. whisper reply (R, `WndWorldControlPlayer.cpp:121-135`)
    if (k.isPending('KeyR') && d.chat.whisperReply()) k.consume('KeyR');
    // 2. chat focus clearing: typing in the chat WINDOW never moves the character (the original cleared W/A/S/D)
    const typing = d.chat.isFocused();
    const held = (code: string): boolean => !typing && k.isDown(code);
    let up = held('KeyW');
    const down = held('KeyS');
    const left = held('KeyA');
    const right = held('KeyD');
    const space = k.isDown('Space');
    // 3. __BS_ADJUST_SYNC: the keyboard overrides a held mouse button
    if (up || down) c.mouse.L.down = false;
    // 4. auto-run
    up = c.autorun.step(up, down, now);
    this.frame = { up, down, left, right, space };
    // 5. create-monster placement (WP-INPUT-12, `placement.ts`)
    this.placementPending =
      this.placement.step(self, up || down || left || right || space) === 'retry';
    // 6. click destination (one per click)
    if (c.mouse.L.down && c.mouse.freeMove) this.clickDestination();
    // 7. movement keys cancel the auto-attack, the pending command and the follow
    if (up || down || left || right || space) {
      c.targeting.autoAttack = false;
      c.commands.clearCmd();
      c.commands.onDestCleared(); // the sim clears the destination with the first state change
      if (!space) c.followId = null;
    }
    // 8. follow upkeep
    this.followUpkeep(self);
    return true;
  }

  /** §6.6: one destination per click. */
  private clickDestination(): void {
    const c = this.c;
    const hit = this.pickGround();
    if (hit === undefined) {
      this.clickPending = true; // no query slot this iteration: retried by the next tick
      return;
    }
    this.clickPending = false;
    if (hit === null) return;
    c.commands.setDestPos(hit.pos, true);
    this.destinations += 1;
    this.lastGround = hit;
    c.mouse.freeMove = false; // holding the button does not steer (KQ-31)
    // moving cancels a casting base motion (`StateModeReq` cancel, doc 45)
    if (c.deps.hooks?.baseMotionActive?.() === true) c.deps.hooks.cancelBaseMotion?.();
    const render = c.deps.render();
    render?.moveMark?.(hit.pos, hit.normal);
    c.followId = null;
  }

  /** The ground point under the pointer (`undefined`: no query slot, retry). */
  pickGround(): GroundPick | null | undefined {
    const c = this.c;
    const cam = c.displayed;
    if (cam === null) return null;
    const vp = c.deps.viewport();
    if (vp.w <= 0 || vp.h <= 0) return null;
    const ray = pickRayLH(cam, c.mouse.pos.x, c.mouse.pos.y, vp.w, vp.h);
    this.lastRay = ray;
    return groundPick((o, dir, maxT, mask) => c.deps.sim.rayCast(o, dir, maxT, mask), ray);
  }

  /** §6.2 step 8: re-issue the destination while the followed player walks away. */
  private followUpkeep(self: SelfInfo): void {
    const c = this.c;
    const id = c.followId;
    if (id === null) return;
    const o = c.deps.entityInfo(id);
    const pos = o?.pos ?? null;
    if (o?.kind !== 'mover' || o.dead || pos === null) {
      c.followId = null;
      return;
    }
    const dx = self.pos[0] - pos[0];
    const dy = self.pos[1] - pos[1];
    const dz = self.pos[2] - pos[2];
    const distSq = dx * dx + dy * dy + dz * dz;
    // the sim drops a destination object when the follower arrives (`is_range_obj` with range 0: the radii count)
    if (c.commands.destObj === id) {
      const reach = 0.8 * self.radius + (0.8 * o.radius + 1);
      if (distSq < reach * reach) c.commands.onDestCleared();
    }
    // `SetDestObj` ignores an unchanged target: the destination is re-issued once the follower arrived and the target walked on
    if (distSq > FOLLOW_REDEST_DIST_SQ) c.commands.setDestObj(id, 0);
  }

  /**
   * `control.runImmediate(kind)` (§6.2): the click handlers run the TypeScript part (steps 1-8) once at event time so that a
   * destination is chosen before the next frame; the sim part runs with the next record. The L+R chord is the exception: the
   * press handlers latch it (`noteChord`) and the next tick runs the ground control once with JUMP (owner #002).
   */
  runImmediate(now: number): void {
    const self = this.c.deps.self();
    if (self === null || self.flying) return;
    this.groundControl(self, now);
  }

  /**
   * The L+R chord check of the press handlers (KQ-31, owner #002). The original evaluated it in the event-time `ControlPlayer`
   * call of `OnLButtonDown/Up`, `OnRButtonUp` and `OnLButtonDblClk` (`WndWorldControlPlayer.cpp:504-511`), not once per tick; the
   * rewrite's ground control never runs while a button is held, so the press latches it and the next decision tick consumes it.
   */
  noteChord(now: number): void {
    const m = this.c.mouse;
    if (m.L.down && m.R.down && now - m.tLDown < LR_JUMP_MS && now - m.tRDown < LR_JUMP_MS) {
      this.chordAt = now;
    }
  }

  // ---- key-up actions (§3.2 column U, R7) --------------------------------------------------------------------------------

  /** The key-up actions of the router (`CWndWorld::OnKeyUp` and the manager). */
  onKeyUp(code: string, mods: Modifiers): void {
    const c = this.c;
    // any key-up clears the nine equip latches (`Neuz.cpp:1415-1419`) and lets the next skill cast at once (`m_SkillTimerStop`)
    this.equipLatch.fill(false);
    c.skill.onKeyUp();
    // ---- housing / guild house (ui-house-quiz): PageUp / PageDown turn the furniture by 45 degrees while it is placed (`Neuz.cpp:1426-1447`)
    if (this.deploy.pageKey(code)) return;
    if (code === 'KeyC' && modifierGateOpen(mods)) {
      // the action slot key (Plan/43 §7.12): the queue starts on the next tick
      c.skill.onActionSlotKey();
      return;
    }
    if (code === 'KeyF' && c.keys.ctrlPushed && mods.ctrl) {
      c.deps.hooks?.toggleFps?.();
      return;
    }
    if (code === 'NumpadMultiply') {
      if (c.deps.windows.allHidden) c.deps.windows.restoreWindows();
      else c.deps.windows.clearAllWindows();
      return;
    }
    if (code === 'ScrollLock') {
      c.camera.requestTelescope();
      return;
    }
    if (code === 'Escape') {
      const map = c.deps.hooks?.worldMap;
      if (map?.shown() === true) map.hide();
      return;
    }
    if (code === 'Tab') {
      // `CWndWorld::OnKeyUp( VK_TAB )`: the guild-war Tab (WP-INPUT-16), independent of the flight (which cycles its own targets on the key-down)
      c.gwTab.press(this.gwTabEnv());
      return;
    }
  }

  /** The ports of one guild-war Tab key-up: the movers drawn in the last frame with their facts, the focus rules, the targeting. */
  private gwTabEnv(): GwTabEnv {
    const c = this.c;
    const d = c.deps;
    return {
      guildWarMap: () => d.hooks?.guildWarMap?.() === true,
      self: () => {
        const s = d.self();
        return s === null ? null : { id: s.id, guildId: s.guildId ?? 0, pos: s.pos };
      },
      candidates: () => {
        const out: GwCandidate[] = [];
        for (const p of d.render()?.visibleObjects?.() ?? []) {
          if (p.kind !== 'mover') continue;
          const o = d.entityInfo(p.id);
          if (o?.kind !== 'mover') continue;
          out.push({
            id: p.id,
            isPlayer: o.isPlayer,
            dead: o.dead,
            transparent: o.transparent,
            guildId: o.guildId ?? 0,
            pos: o.pos,
            radius: o.radius,
          });
        }
        return out;
      },
      focusable: (id) => {
        const o = d.entityInfo(id);
        return o !== null && !o.transparent && !o.dead;
      },
      setFocus: (id) => {
        c.targeting.setFocus(id);
      },
    };
  }

  // ---- the record (§6.1) -----------------------------------------------------------------------------------------------

  private buildRecord(self: SelfInfo): Int32Array {
    const c = this.c;
    const m = c.mouse;
    const k = c.keys;
    const f = this.frame;
    const r = this.rec;
    let b = 0;
    if (!this.controlRuns) {
      b |= INPUT_BUTTONS.BLOCKED;
    } else if (self.flying) {
      b |= c.flight.recordBits();
      if (m.L.down) b |= INPUT_BUTTONS.LMB_DOWN;
    } else {
      if (f.up) b |= INPUT_BUTTONS.UP;
      if (f.down) b |= INPUT_BUTTONS.DOWN;
      if (f.left) b |= INPUT_BUTTONS.LEFT;
      if (f.right) b |= INPUT_BUTTONS.RIGHT;
      if (f.space || this.chordJump) b |= INPUT_BUTTONS.JUMP;
      if (this.chordJump) {
        // one JUMP per chord; the sim's JUMP clears the command and the destination, the mirror follows (the chord leaves the
        // auto-attack and the follow alone, like the original; only Space's step 7 ends them)
        this.chordJumps += 1;
        c.commands.onDestCleared();
      }
      // X rising edge (`s_bWalk2`): evaluated only in the ticks that run the control, like the original
      const x = k.isHeld('KeyX');
      if (x && !this.walkLatch) b |= INPUT_BUTTONS.WALK_TOGGLE;
      this.walkLatch = x;
      if (m.L.down) b |= INPUT_BUTTONS.LMB_DOWN;
    }
    if (m.L.down) b |= INPUT_BUTTONS.LMB_HELD;
    if (c.targeting.autoAttack) b |= INPUT_BUTTONS.AUTO_ATTACK;
    if (c.targeting.nextSkill === NEXTSKILL_NONE) b |= INPUT_BUTTONS.NEXT_SKILL_NONE;
    r[INPUT_SLOT.BUTTONS] = b;
    this.lastButtons = b;
    // the flight steering pixels (mode 0 / mode 1, Plan/43 §9.3); a ground record carries none
    const steer = this.controlRuns && self.flying;
    r[INPUT_SLOT.MOUSE_DX] = steer ? c.flight.frame.dx : 0;
    r[INPUT_SLOT.MOUSE_DY] = steer ? c.flight.frame.dy : 0;
    r[INPUT_SLOT.LEVEL] = self.level;
    r[INPUT_SLOT.REQ_FLAGS] = c.reqFlags;
    return r;
  }

  /** The neutral record of a hidden tab (§2.4): nothing held, `BLOCKED`, auto-attack off. */
  neutralRecord(level: number): Int32Array {
    const r = this.rec;
    r[INPUT_SLOT.BUTTONS] = INPUT_BUTTONS.BLOCKED | INPUT_BUTTONS.NEXT_SKILL_NONE;
    r[INPUT_SLOT.MOUSE_DX] = 0;
    r[INPUT_SLOT.MOUSE_DY] = 0;
    r[INPUT_SLOT.LEVEL] = level;
    r[INPUT_SLOT.REQ_FLAGS] = 0;
    return r;
  }

  /** World entry (`OpenField`): auto-run ends, nothing is pending. */
  reset(): void {
    this.focusWorldPending = true;
    this.walkLatch = false;
    this.clickPending = false;
    this.placementPending = false;
    this.chordAt = Number.NEGATIVE_INFINITY;
    this.chordJump = false;
    this.placement.reset();
    this.deploy.reset();
    this.equipLatch.fill(false);
    this.c.autorun.reset();
    this.frame = { up: false, down: false, left: false, right: false, space: false };
  }

  /** Whether the chat WINDOW is focused or the vendor windows are open (used by the probe). */
  get typing(): boolean {
    return this.c.deps.windows.focusedApp() === APP_COMMUNICATION_CHAT;
  }
}
