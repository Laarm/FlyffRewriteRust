// `createInput(deps)`: the input layer of the game (Plan/43), composed from its modules and exposed as the `InputApi` port of
// the game loop (`game/ports.ts`): `worldTick` before the sim step, `camera` and `hover` in the render part of the iteration.
//
//   DOM keyboard   -> InputRouter -> KeyState (+ key-up actions, the chat/modal rules)
//   DOM pointer    -> PointerController -> WorldClicks (selection, click-to-move, follow, camera drag)
//   loop           -> worldTick (Esc chain, hotkeys, control decision, the sim input record)
//                  -> camera (CBackCamera per stepped frame, one camera_check_bound) -> hover (pick, highlight, cursor)

import { create } from '@bufbuild/protobuf';
import { EXT_SELFCHGPARAMET } from '../gen/defs/define';
import { IK3_CHEERSTICK, IK3_STAFF, IK3_WAND } from '../gen/defs/itemkind';
import { SI_KNT_HERO_DRAWING } from '../gen/defs/skill';
import { TID_GAME_CAMERA_ZOOM_LEVEL } from '../gen/defs/text';
// ---- housing / guild house (ui-house-quiz)
import { WI_WORLD_MINIROOM } from '../gen/defs/world';
import { SetTargetReqSchema } from '../gen/proto/flyff/v1/combat_pb';
import { CHATSTY } from '../ui/chat/styles';
import type {
  CameraSample,
  InputApi,
  SkillInputPort,
  TargetingPort,
  TargetView,
} from '../game/ports';
import { OBJACT, STA } from '../game/combat/constants';
import { bitsToFloat } from '../wasm/simTypes';
import { AttackController } from './attack';
import { SkillController } from './skill';
import { AutoRun } from './autorun';
import { ChargeGauge } from './charge';
import { FlightControl, type FlightSnapshot } from './flight';
import { GuildWarTab, type GwTabSnapshot } from './guildWarTab';
import { BackCamera, type BoundResult } from './camera/backCamera';
import { CameraDriver, TELESCOPE_NOTICE_COLOR } from './camera/cameraControls';
import { LocalCommands } from './commands';
import { installBrowserGuards } from './guards';
import { KeyState, type KeyCode } from './keys';
import {
  PointerController,
  WorldMouse,
  type CanvasLike,
  type PointerEventLike,
  type WheelLike,
} from './pointer';
import type { InputDeps } from './ports';
import { Picker } from './picker';
import { InputRouter, type KeyEventLike } from './router';
import { NEXTSKILL_NONE, Targeting } from './targeting';
import { WheelNormalizer } from './wheel';
import { WorldClicks } from './worldClicks';
import { REQ_USESKILL, WorldTick, type InputCore } from './worldTick';

/** The sim events the input layer consumes (`SimEventRecord` satisfies it). */
export interface InputSimEvent {
  readonly name: string;
  readonly objId: number;
  readonly words: readonly number[];
}

/** A synthetic event of the debug surface (`__flyffDebug.input.inject`). */
export interface InjectKey {
  type: 'keydown' | 'keyup';
  code: KeyCode;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
}
export interface InjectMouse {
  type: 'mousedown' | 'mouseup' | 'mousemove';
  button?: 0 | 1 | 2;
  /** Canvas pixels. */
  x: number;
  y: number;
  /** `movementX/Y` of the event (what a Pointer Lock supplies; used while the lock is held). */
  movement?: { x: number; y: number };
  /**
   * The `PointerEvent.buttons` mask of the event (L 1, R 2, M 4). The default follows the downs and ups of the same `inject` call
   * (a later call starts with no button down): a sequence that spans calls passes the mask it means.
   */
  buttons?: number;
  /** The modifier keys of the event (`PointerEvent.ctrlKey` ...): the personal-shop click reads Alt and Ctrl. */
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
}
export interface InjectWheel {
  type: 'wheel';
  deltaY: number;
  deltaMode?: number;
  wheelDeltaY?: number;
}
export type InjectEvent = InjectKey | InjectMouse | InjectWheel;

function isKey(e: InjectEvent): e is InjectKey {
  return e.type === 'keydown' || e.type === 'keyup';
}
function isWheel(e: InjectEvent): e is InjectWheel {
  return e.type === 'wheel';
}

export interface InputInstance extends InputApi {
  /** The loop's world methods, always present on the real thing. */
  worldTick(nowMs: number): unknown;
  camera(due: number, alpha: number, view: unknown): CameraSample | null;
  hover(): void;
  /** The render-facing view of the targeting (always present on the real thing). */
  targetView(): TargetView;
  /** The targeting state for the message handlers of 17 (`SetTargetNtf`, `ClearTarget`, `MoverDeath`). */
  readonly targeting: TargetingPort;
  /** The skill side of the input layer for the skill runtime of 17 (`UseSkillNtf`, `ClearUseSkill`, `EndSkillQueueNtf`). */
  readonly skills: SkillInputPort;
  readonly core: InputCore;
  readonly router: InputRouter;
  readonly tick: WorldTick;
  readonly clicks: WorldClicks;
  readonly picker: Picker;
  /** Installs the DOM listeners: the keyboard on `win`, the pointer on the world canvas. */
  attach(win: Window, canvas: CanvasLike): void;
  detach(): void;
  /** A sim event of this frame (`LmbRelease`, `CameraQuake`, `AttrQuake`, `Notice`, `CastCancel`). */
  onSimEvent(e: InputSimEvent): void;
  /** World entry (`OpenField`) and world change: auto-run and every selection reset, the camera locks. */
  onWorldEntered(): void;
  /** The world is left. */
  onWorldLeft(): void;
  /** Debug surface (`window.__flyffDebug.input`, dev builds and `?e2e=1`). */
  readonly debug: InputDebug;
}

export interface InputDebug {
  keys(): ReturnType<KeyState['snapshot']>;
  mouse(): ReturnType<WorldMouse['snapshot']>;
  targeting(): Record<string, unknown>;
  /** The attack controller: counters and the invocations waiting for a query slot, the charge gauge. */
  attack(): {
    stats: Record<string, number>;
    pending: number;
    gauge: number | null;
    powerTick: number;
  };
  /** The skill controller: counters, the waiting invocations, the skill timer, the action slot target. */
  skill(): {
    stats: Record<string, number>;
    pending: number;
    skillTimerStop: boolean;
    queueTarget: number;
    reqUseSkill: boolean;
  };
  /** The flight controls: mouse mode, Pointer Lock counters, tracking, the fly targets, the last control step. */
  flight(): FlightSnapshot;
  /** The guild-war Tab: the cycle counter, the list of the last key-up and the mover it focused (WP-INPUT-16). */
  gwTab(): GwTabSnapshot;
  camera(): Record<string, unknown>;
  lastPick(): { id: number; kind: string; t: number } | null;
  tick(): {
    controlRuns: boolean;
    ticks: number;
    lastEsc: string | null;
    autoRun: boolean;
    buttons: number;
    /** JUMPs sent because of an L+R chord (one per chord, owner #002). */
    chordJumps: number;
  };
  /** Click destinations written and the last ground pick (LH). */
  ground(): {
    destinations: number;
    last: { pos: number[]; normal: number[]; kind: number } | null;
    ray: { origin: number[]; dir: number[] } | null;
  };
  /** The last notices the input layer printed (telescope, run/walk), oldest first. */
  notices(): { text: string; color: number; style: number }[];
  /** A synthetic key / mouse / wheel sequence through the same handlers as the DOM events. */
  inject(events: readonly InjectEvent[]): void;
  /** A fake clock (`null` restores the real one) for the timer tests. */
  setTime(ms: number | null): void;
}

const MOVE_KEYS: readonly KeyCode[] = ['KeyW', 'KeyS', 'KeyA', 'KeyD'];

export function createInput(rawDeps: InputDeps): InputInstance {
  const notices: { text: string; color: number; style: number }[] = [];
  const deps: InputDeps = {
    ...rawDeps,
    notice: (text, color, style) => {
      notices.push({ text, color, style });
      if (notices.length > 20) notices.shift();
      rawDeps.notice?.(text, color, style);
    },
  };
  let clockOverride: number | null = null;
  const clock = (): number => Math.floor(clockOverride ?? deps.now());
  const holder: { core?: InputCore } = {};
  const kindOf = (id: number): ReturnType<InputDeps['entityInfo']> => deps.entityInfo(id);

  const keys = new KeyState();
  const mouse = new WorldMouse();
  const targeting = new Targeting(
    {
      isMover: (id) => kindOf(id)?.kind === 'mover',
      isHousing: (id) => kindOf(id)?.housing === true,
      // ---- housing / guild house (ui-house-quiz)
      isGuildHouse: (id) => kindOf(id)?.guildHouse === true,
      isLive: (id) => {
        const o = kindOf(id);
        return o?.kind === 'mover' && !o.dead;
      },
      exists: (id) => kindOf(id) !== null,
      isPeaceful: (id) => kindOf(id)?.peaceful === true,
      isTransparent: (id) => kindOf(id)?.transparent === true,
      isDead: (id) => kindOf(id)?.dead === true,
      selfId: () => deps.self()?.id ?? 0,
      flying: () => deps.self()?.flying === true,
      guildWarMap: () => deps.hooks?.guildWarMap?.() ?? false,
      targetSave: () => deps.features()?.targetsave !== false,
    },
    {
      // `SendSetTarget(target, clear)`: the claim on a target (kill-steal protection) and the party-skill target
      sendTarget: (target, clear) => {
        // legacy-send: SendSetTarget
        deps.send?.({ case: 'setTargetReq', value: create(SetTargetReqSchema, { target, clear }) });
      },
      // ---- housing / guild house (ui-house-quiz)
      guildFurniture: (id) => deps.hooks?.guildFurniture?.(id) ?? false,
      clearQuestDestination: () => {
        const q = deps.hooks?.questDestination;
        if (q?.active() === true) q.clear();
      },
    },
  );
  const commands = new LocalCommands({
    sim: deps.sim,
    isPlayer: (id) => kindOf(id)?.isPlayer === true,
    ...(deps.hand ? { hand: () => deps.hand?.() ?? null } : {}),
    self: () => {
      const me = deps.self();
      return me === null ? null : { dead: me.dead, actAttack: (me.state & STA.ATK_ALL) !== 0 };
    },
    bullet: () => deps.features()?.no_arrows_poster === true || (deps.bulletEquipped?.() ?? true),
  });
  const autorun = new AutoRun();
  const camera = new BackCamera(deps.random ? { random: deps.random } : {});

  const movementHeld = (): boolean => !deps.chat.isFocused() && keys.anyHeld(MOVE_KEYS);
  const driver = new CameraDriver({
    camera,
    mouse,
    keys,
    options: () => deps.options(),
    subject: (view) => deps.cameraSubject(view),
    focusPos: () => {
      const f = holder.core?.targeting.focus ?? null;
      if (f === null) return null;
      const o = kindOf(f);
      return o?.kind === 'mover' ? o.pos : null;
    },
    tracking: () => holder.core?.flight.tracking ?? false,
    movementHeld,
    deployActive: () => deps.hooks?.deploy?.active() ?? deps.hooks?.deployActive?.() ?? false,
    noCollision: () => deps.hooks?.gc1to1?.participant() ?? false,
    checkBound: (eye, lookAt): BoundResult => deps.sim.cameraCheckBound(eye, lookAt),
    telescopeNotice: (level) => {
      const template = deps.text?.(TID_GAME_CAMERA_ZOOM_LEVEL) ?? '%dX Zoom';
      deps.notice?.(template.replace('%d', String(level)), TELESCOPE_NOTICE_COLOR, CHATSTY.GENERAL);
    },
  });

  /** The system line of a client text id (`TID_GAME_BLOCKTARGETING`, the run / walk notices). */
  const notify = (tid: number): void => {
    const text = deps.text?.(tid);
    if (text !== undefined && text !== '') {
      deps.notice?.(text, deps.textColor?.(tid) ?? 0xffffffff, CHATSTY.SYSTEM_CLIENT);
    }
  };

  const charge = new ChargeGauge();
  // `attack` needs the core it lives in: the controller reads everything through `holder.core`
  const attackProxy: {
    ctl?: AttackController;
    skill?: SkillController;
    flight?: FlightControl;
  } = {};
  const core: InputCore = {
    deps,
    keys,
    mouse,
    targeting,
    commands,
    autorun,
    camera: driver,
    displayed: null,
    followId: null,
    reqFlags: 0,
    charge,
    gwTab: new GuildWarTab(),
    notify,
    get attack(): AttackController {
      if (!attackProxy.ctl) throw new Error('the attack controller is not built yet');
      return attackProxy.ctl;
    },
    get skill(): SkillController {
      if (!attackProxy.skill) throw new Error('the skill controller is not built yet');
      return attackProxy.skill;
    },
    get flight(): FlightControl {
      if (!attackProxy.flight) throw new Error('the flight controls are not built yet');
      return attackProxy.flight;
    },
  };
  attackProxy.ctl = new AttackController(core);
  attackProxy.skill = new SkillController(core);
  attackProxy.flight = new FlightControl(core, deps.pointerLock);
  holder.core = core;

  const tick = new WorldTick(core);
  core.pickGround = () => tick.pickGround();
  const picker = new Picker(core, {
    worldBlocked: () => clicks.worldBlocked(),
  });
  const clicks: WorldClicks = new WorldClicks(core, tick, picker);

  const router = new InputRouter({
    keys,
    mode: () => deps.mode(),
    windows: deps.windows,
    focusChat: () => deps.chat.focusInput(),
    ...(deps.hooks?.statConfirm ? { statConfirm: deps.hooks.statConfirm } : {}),
    // the browser's own Pointer Lock exit (Escape) is not an Esc-chain press, and Escape leaves the free steering mode (§9.3)
    // ---- housing / guild house (ui-house-quiz): Esc ends a placement first (`Neuz.cpp:1384-1396`)
    onEscapeDown: () => core.flight.onEscapeDown(clock()) || tick.deploy.escape(),
    onGameKeyDown: (code) => {
      core.skill.onKeyDown(); // `m_SkillTimerStop = FALSE` (a held skill key casts once per second)
      if (MOVE_KEYS.includes(code)) targeting.onMovementKeyDown();
    },
    onGameKeyUp: (code, mods) => {
      tick.onKeyUp(code, mods);
    },
    onEditFocused: () => {
      mouse.dragDX = 0;
      mouse.dragDY = 0;
    },
  });

  const wheel = new WheelNormalizer();
  const pointer = new PointerController({
    mouse,
    handlers: clicks,
    now: clock,
    wheelNotches: (e: WheelLike, t) => wheel.feed(e, t),
    active: () => deps.mode() === 'game',
    // under Pointer Lock the position is frozen and `movementX/Y` steers the flight (§9.3)
    locked: () => core.flight.lock.locked,
    // the press, move and wheel that the UI consumed (a window, a buff icon, a modal) never reach the world (BFIX1 #004)
    ...(deps.uiClaimed ? { uiClaimed: (e: object) => deps.uiClaimed?.(e) === true } : {}),
  });

  const debug: InputDebug = {
    keys: () => keys.snapshot(),
    mouse: () => mouse.snapshot(),
    targeting: () => targeting.snapshot(),
    attack: () => ({
      stats: { ...core.attack.stats },
      pending: core.attack.pending,
      gauge: tick.gauge,
      powerTick: charge.powerTick,
    }),
    skill: () => ({
      stats: { ...core.skill.stats },
      pending: core.skill.pending,
      skillTimerStop: core.skill.skillTimerStop,
      queueTarget: core.skill.queueTarget,
      reqUseSkill: (core.reqFlags & REQ_USESKILL) !== 0,
    }),
    flight: () => core.flight.snapshot(),
    gwTab: () => core.gwTab.snapshot(),
    camera: () => camera.snapshot(),
    lastPick: () => (picker.lastPick ? { ...picker.lastPick } : null),
    tick: () => ({
      controlRuns: tick.controlRuns,
      ticks: tick.ticks,
      lastEsc: tick.lastEsc,
      autoRun: autorun.active,
      buttons: tick.lastButtons,
      chordJumps: tick.chordJumps,
    }),
    ground: () => ({
      destinations: tick.destinations,
      last: tick.lastGround
        ? {
            pos: [...tick.lastGround.pos],
            normal: [...tick.lastGround.normal],
            kind: tick.lastGround.kind,
          }
        : null,
      ray: tick.lastRay ? { origin: [...tick.lastRay.origin], dir: [...tick.lastRay.dir] } : null,
    }),
    notices: () => notices.map((n) => ({ ...n })),
    setTime: (ms) => {
      clockOverride = ms;
    },
    inject: (events) => {
      let buttons = 0;
      const base = (): PointerEventLike & WheelLike => ({
        pointerId: 1,
        pointerType: 'mouse',
        button: 0,
        buttons,
        clientX: 0,
        clientY: 0,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        metaKey: false,
        deltaY: 0,
        deltaMode: 0,
        preventDefault: () => undefined,
      });
      for (const ev of events) {
        if (isKey(ev)) {
          const k: KeyEventLike = {
            code: ev.code,
            ctrlKey: ev.ctrl === true,
            altKey: ev.alt === true,
            shiftKey: ev.shift === true,
            metaKey: false,
            preventDefault: () => undefined,
          };
          if (ev.type === 'keydown') router.onKeyDown(k);
          else router.onKeyUp(k);
        } else if (isWheel(ev)) {
          const w: WheelLike = {
            ...base(),
            deltaY: ev.deltaY,
            deltaMode: ev.deltaMode ?? 0,
            ...(ev.wheelDeltaY !== undefined ? { wheelDeltaY: ev.wheelDeltaY } : {}),
          };
          const n = wheel.feed(w, clock());
          if (n !== 0) clicks.wheel(n);
        } else {
          const button = ev.button ?? 0;
          const bit = [1, 4, 2][button] ?? 1;
          if (ev.type === 'mousedown') buttons |= bit;
          if (ev.type === 'mouseup') buttons &= ~bit;
          if (ev.buttons !== undefined) buttons = ev.buttons;
          // the canvas rect is the origin here: the injected coordinates are canvas pixels
          const p: PointerEventLike = {
            ...base(),
            button,
            buttons,
            clientX: ev.x,
            clientY: ev.y,
            ctrlKey: ev.ctrl === true,
            altKey: ev.alt === true,
            shiftKey: ev.shift === true,
            ...(ev.movement ? { movementX: ev.movement.x, movementY: ev.movement.y } : {}),
          };
          if (ev.type === 'mousedown') pointer.onPointerDown(p);
          else if (ev.type === 'mouseup') pointer.onPointerUp(p);
          else pointer.onPointerMove(p);
        }
      }
    },
  };
  let attached: {
    win: Window;
    canvas: CanvasLike;
    guards: (() => void) | null;
    unlock: () => void;
  } | null = null;

  /** The object `targetView()` hands out (reused: valid until the next call). */
  const view: { -readonly [K in keyof TargetView]: TargetView[K] } = {
    focus: null,
    renderTarget: null,
    nextTarget: null,
    autoAttack: false,
    gauge: null,
    flyTargets: [],
  };

  const input: InputInstance = {
    core,
    router,
    tick,
    clicks,
    picker,
    debug,

    // ---- render-audio-m7 (doc 41 section 17.1): the camera quake of a boss attack (`CCamera::SetQuake`); the host gates it by `features.no_quake`
    quake(ticks: number, size: number): void {
      camera.setQuake(Math.trunc(ticks), size);
    },

    flushImmediate(): void {
      // the pointer handlers ran at event time; the attack invocations that waited for a query slot run first (Plan/43 §2.2)
      core.attack.pump();
      core.skill.pump();
    },

    targeting: targetingPort(core),
    skills: skillPort(core),

    targetView(): TargetView {
      view.focus = targeting.focus;
      view.renderTarget = targeting.renderTarget;
      view.nextTarget = targeting.nextTarget;
      view.autoAttack = targeting.autoAttack;
      view.gauge = tick.gauge;
      view.flyTargets = core.flight.targets;
      return view;
    },

    release(): void {
      keys.clear();
      mouse.releaseAll();
      wheel.reset();
      pointer.onLostCapture();
      core.flight.leaveMouseMode(); // a blur or a hidden tab ends the free steering and its lock (§2.4)
    },

    worldTick(nowMs: number): unknown {
      if (deps.mode() !== 'game') return null;
      if (deps.hidden?.() === true) {
        // a hidden tab: nothing is held, the character stands, auto-run is only suspended (§2.4)
        return tick.neutralRecord(deps.self()?.level ?? 1);
      }
      return tick.tick(Math.floor(clockOverride ?? nowMs));
    },

    camera(due: number, alpha: number, view: unknown): CameraSample | null {
      // also while the world loads or reconnects (`none`): the frozen world keeps its camera; no subject, no camera
      if (deps.mode() === 'title') return null;
      driver.process(due, view);
      const st = driver.sample(alpha);
      if (st === null) return null;
      core.displayed = { pos: st.pos, lookAt: st.lookAt, up: st.up, fovDiv: st.fovDiv };
      const s = driver.subject;
      const listener = s ? s.pos : st.lookAt;
      return { listener: [listener[0], listener[1], listener[2]], yaw: st.yawDeg, camera: st };
    },

    hover(): void {
      picker.hover();
      core.flight.updateTargets(); // the fly-target list of the frame (§9.4)
    },

    attach(win: Window, canvas: CanvasLike): void {
      input.detach();
      router.attach(win);
      pointer.attach(canvas);
      // the document-wide pointer guards (the contextmenu of a drag that ends over a window)
      const guards =
        typeof win.document === 'object'
          ? installBrowserGuards(win.document, () => deps.mode() !== 'none')
          : null;
      // the Pointer Lock of the flight steering: the browser reports its acquisition, its loss and a refusal on the document
      const doc = typeof win.document === 'object' ? win.document : null;
      const onLock = (): void => {
        const locked = doc?.pointerLockElement === (canvas as unknown);
        core.flight.onLockChange(locked, clock());
      };
      const onLockError = (): void => {
        core.flight.onLockError();
      };
      doc?.addEventListener('pointerlockchange', onLock);
      doc?.addEventListener('pointerlockerror', onLockError);
      const unlock = (): void => {
        doc?.removeEventListener('pointerlockchange', onLock);
        doc?.removeEventListener('pointerlockerror', onLockError);
      };
      attached = { win, canvas, guards, unlock };
    },

    detach(): void {
      if (attached === null) return;
      router.detach();
      pointer.detach();
      attached.guards?.();
      attached.unlock();
      attached = null;
    },

    onSimEvent(e: InputSimEvent): void {
      switch (e.name) {
        case 'LmbRelease':
          mouse.L.down = false; // a collision ended the click move: no up handler (31 §15.2)
          break;
        case 'CameraQuake': {
          // ---- render-audio-m7: `client/events.rs` exports `frames` as a plain u32 (only `scale` is a float bit pattern); the read as float bits made every such quake last 0
          // ticks, which is also what kept the two ungated sim emitters (the monster run hit, the `MA_QUAKE` attribute) silent. Now gated like `AttrQuake` (PL-134, F-333)
          if (deps.features()?.no_quake !== false) break;
          const frames = e.words[0] ?? 0;
          const scale = bitsToFloat(e.words[1] ?? 0);
          camera.setQuake(Math.trunc(frames), scale);
          break;
        }
        case 'AttrQuake':
          if (deps.features()?.no_quake === false) camera.setQuake(15, 0.06);
          break;
        case 'Notice':
          notify(e.words[0] ?? 0);
          break;
        case 'CastCancel':
          deps.hooks?.cancelBaseMotion?.();
          break;
        case 'CmdArrived':
          if ((e.words[0] ?? 0) === OBJACT.USESKILL) core.skill.onCmdArrived(e.words[1] ?? 0);
          else core.attack.onCmdArrived(e.words[0] ?? 0, e.words[1] ?? 0);
          break;
        case 'CmdAborted':
          core.attack.onCmdAborted();
          break;
        case 'LocalAttack':
          core.attack.onLocalAttack(e.words[0] ?? 0, e.words[1] ?? 0, e.words[2] ?? 0);
          break;
        default:
          break;
      }
    },

    onWorldEntered(): void {
      keys.clear();
      mouse.releaseAll();
      tick.reset();
      targeting.reset();
      core.attack.clear();
      core.skill.reset();
      core.flight.reset();
      core.reqFlags = 0;
      charge.restart(0);
      commands.onDestCleared();
      core.followId = null;
      core.displayed = null;
      driver.onWorldChanged();
      // ---- housing / guild house (ui-house-quiz): the mini room's camera clamps (`DPClient.cpp:2456-2466`, after `m_camera.Reset()`)
      camera.mode = deps.sim.worldId === WI_WORLD_MINIROOM ? 'myroom' : 'normal';
    },

    onWorldLeft(): void {
      keys.clear();
      mouse.releaseAll();
      targeting.reset();
      core.attack.clear();
      core.skill.reset();
      core.flight.reset();
      core.reqFlags = 0;
      core.displayed = null;
    },
  };

  return input;
}

/** The port the combat message handlers use (`ctx.input.targeting`): the targeting state of the input layer. */
function targetingPort(core: InputCore): TargetingPort {
  const t = core.targeting;
  return {
    focus: () => t.focus,
    onServerTarget: (id) => {
      t.onServerTarget(id);
    },
    onClearTarget: () => {
      t.onClearTarget();
    },
    onMoverDeath: (victim, attacker) => {
      t.onMoverDeath(victim, attacker);
      const self = core.deps.self()?.id ?? 0;
      if (self !== 0 && attacker === self) {
        // the killer's hit target and walk to it end (`SetObjHit(NULL_ID)`, `ClearDestObj`)
        core.commands.clearDest();
        core.attack.clear();
      }
      if (self !== 0 && victim === self) {
        t.autoAttack = false;
        core.attack.clear();
      }
    },
    onMoverSpawned: (id) => {
      t.onMoverSpawned(id);
    },
  };
}

/** The port the skill runtime of 17 uses (`ctx.input.skills`): the answers of the server reach the input layer here. */
function skillPort(core: InputCore): SkillInputPort {
  return {
    clearReqUseSkill: () => {
      core.skill.clearReqUseSkill();
    },
    onClearUseSkill: () => {
      // `OnClearUseSkill`: the request is answered, the action slot stops, the reservation ends
      core.skill.clearReqUseSkill();
      core.skill.cancel();
      core.targeting.nextSkill = NEXTSKILL_NONE;
    },
    onEndSkillQueue: () => {
      core.skill.cancel();
    },
    onOwnSkillStarted: (skillId, exeTarget) => {
      // `OnUseSkill` (ref/09a 5.3): the world auto-attack starts with a skill when the option is on, the weapon is no staff,
      // cheer stick or wand, the skill is no self-buff and not Hero Drawing
      const hand = core.deps.hand?.() ?? null;
      if (
        core.deps.options().autoAttack &&
        hand !== null &&
        hand.kind3 !== IK3_STAFF &&
        hand.kind3 !== IK3_CHEERSTICK &&
        hand.kind3 !== IK3_WAND &&
        exeTarget !== EXT_SELFCHGPARAMET &&
        skillId !== SI_KNT_HERO_DRAWING
      ) {
        core.targeting.autoAttack = true;
      }
    },
    clearNextSkill: () => {
      core.targeting.nextSkill = NEXTSKILL_NONE;
    },
    notify: (tid) => {
      core.notify(tid);
    },
    useSkillShortcut: (slot) => {
      core.skill.useSkillShortcut(slot);
    },
    useActionSlot: () => {
      core.skill.onActionSlotKey();
    },
  };
}
