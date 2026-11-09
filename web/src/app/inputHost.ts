// The input layer of the game (Plan/43; WP-INPUT-01..08, boot): `installInputHost(ctx, { canvas })` builds `createInput` over the
// narrow ports of `input/ports.ts` from the client context (the window manager, taskbar and chat of 42, the entity table and
// stores, the render view, the options store), installs the DOM listeners (the keyboard on `window`, the pointer on the
// `#world` canvas), subscribes the sim events it consumes, resets itself on world entry and exit, and publishes the debug
// surface as `__flyffDebug.input`. The loop talks to the result through `ctx.input` (`InputApi`).

import { defineOptionsStore, setOptionByPath } from '../features/options';
import { fullUi } from '../features/common/module';
import type { RenderViewReader } from '../game/renderView';
import { APP_COMMUNICATION_CHAT } from '../gen/defs/resdata';
// ---- housing / guild house (ui-house-quiz)
import { CK1_GUILD_HOUSE, CK1_HOUSING } from '../gen/defs/itemkind';
import { PARTS_RWEAPON } from '../gen/defs/neuz';
import { RENDER_VIEW } from '../gen/sim/wire';
import { createInput, type InputInstance, type InputSimEvent } from '../input/input';
import type { CameraSubject } from '../input/camera/backCamera';
import type { PointerLockPort } from '../input/flight';
import type {
  ChatPort,
  InputDeps,
  InputFeatures,
  HandItem,
  InputHooks,
  ObjectInfo,
  SelfInfo,
  TaskbarPort,
  WorldWindows,
} from '../input/ports';
import { EntityKind } from '../game/entities';
import { skillsRuntime } from '../game/combat/skills';
import type { ClientContext } from './context';
import { inputProvidersOf } from './inputProviders';
import type { Vec3LH } from '../render/types';
import { isUiClaimed } from '../ui/wm/pointerClaims';

const MV = RENDER_VIEW.mover;
const FLAGS = RENDER_VIEW.moverFlags;
const STRIDE = RENDER_VIEW.MOVER_STRIDE;
const REPORT_ANGLE_X = 16;
/** `m_dwMode` bit of `TRANSPARENT_MODE` (`MoverVisualState.mode`). */
const MODE_TRANSPARENT = 0x2;
/** `OBJSTAF_ACC`, `OBJSTAF_TURBO` of the state-flag word of the render view. */
const STAF_ACC = 0x10;
const STAF_TURBO = 0x80;
/** `OBJSTAF_SIT`, `OBJSTAF_COMBAT`. */
const STAF_SIT = 0x04;
const STAF_COMBAT = 0x01;
/** `WI_WORLD_GUILDWAR` (`flyff_common::defs::world`): a dead next target is skipped on this map (43 section 7.3). */
const WI_WORLD_GUILDWAR = 202;
/** The sim events the input layer consumes. */
const SIM_EVENTS: ReadonlySet<string> = new Set([
  'LmbRelease',
  'CameraQuake',
  'AttrQuake',
  'Notice',
  'CastCancel',
  // the attacks of the own character (WP-INPUT-10): the arrival of a command, the next combo step, an aborted shot
  'CmdArrived',
  'CmdAborted',
  'LocalAttack',
]);

export interface InputHostOptions {
  /** The world canvas (`#world`). */
  readonly canvas: HTMLCanvasElement;
  /** Test seam: the window that receives the keyboard (default `window`). */
  readonly win?: Window;
}

export interface InputHost {
  readonly input: InputInstance;
  dispose(): void;
}

/** The view reader when the bridge produced a valid one. */
function viewOf(ctx: ClientContext): RenderViewReader | null {
  const v = ctx.sim.view;
  return v?.valid === true ? v : null;
}

/** `WorldWindows` over the window manager of 42 (inert answers while the UI does not exist yet). */
function windowsOf(ctx: ClientContext): WorldWindows {
  const wm = () => fullUi(ctx)?.wm ?? null;
  return {
    isEditFocused: () => wm()?.isEditFocused() ?? false,
    modalOpen: () => wm()?.modalOpen() ?? false,
    isStateConfirmOpen: () => wm()?.isStateConfirmOpen() ?? false,
    isLogoutPending: () => wm()?.isLogoutPending() ?? false,
    chatOpen: () => wm()?.isOpen(APP_COMMUNICATION_CHAT) ?? false,
    keyDown: (code, info) => wm()?.keyDown(code, info) ?? false,
    keyUp: (code, info) => wm()?.keyUp(code, info) ?? false,
    isOpen: (app) => wm()?.isOpen(app) ?? false,
    focusedApp: () => wm()?.focusedApp() ?? null,
    anyOpen: (apps) => wm()?.anyOpen(apps) ?? false,
    isVendorWindowOpen: () => wm()?.isVendorWindowOpen() ?? false,
    isDraggingShortcut: () => wm()?.isDraggingShortcut() ?? false,
    cancelDragUnlessFrom: (app) => {
      wm()?.cancelDragUnlessFrom(app);
    },
    topClosableWindow: () => wm()?.topClosableWindow() ?? null,
    closeTopWindow: () => wm()?.closeTopWindow() ?? 'none',
    openStartMenu: () => {
      wm()?.openStartMenu(true);
    },
    focusWorld: () => {
      wm()?.root.setFocus();
    },
    clearAllWindows: () => {
      wm()?.clearAllWindows();
    },
    restoreWindows: () => {
      wm()?.restoreWindows();
    },
    get allHidden(): boolean {
      return wm()?.allHidden ?? false;
    },
    objectExecutorApplet: (app) => {
      wm()?.objectExecutorApplet(app);
    },
  };
}

function taskbarOf(ctx: ClientContext): TaskbarPort {
  const tb = () => fullUi(ctx)?.taskbar ?? null;
  return {
    executeItemSlot: (i) => {
      tb()?.executeItemSlot(i);
    },
    setItemPage: (n) => {
      tb()?.setItemPage(n);
    },
    slotIsEquipItem: (i) => tb()?.slotIsEquipItem(i) ?? false,
  };
}

function chatOf(ctx: ClientContext): ChatPort {
  const ui = () => fullUi(ctx);
  return {
    focusInput: () => ui()?.chat.focusInput() ?? false,
    isOpen: () => ui()?.wm.isOpen(APP_COMMUNICATION_CHAT) ?? false,
    isFocused: () => ui()?.wm.focusedApp() === APP_COMMUNICATION_CHAT,
    whisperReply: () => {
      const u = ui();
      const name = u?.chat.ports().view.lastWhisperer() ?? null;
      const w = u?.chat.chatWindow() ?? null;
      if (!u || !w || name === null || name === '') return false;
      // `/whisper <name> ` into the chat edit (`WndWorldControlPlayer.cpp:121-135`)
      w.setFocus();
      w.edit.setString(`/whisper ${name} `);
      w.edit.setFocus();
      w.edit.caretToEnd();
      return true;
    },
  };
}

/** The input state of `OptionsV1` (45 §27.1) as the input layer reads it. */
function optionsOf(ctx: ClientContext): InputDeps['options'] {
  const store = defineOptionsStore(ctx.stores.registry);
  return () => {
    const o = store.getState();
    return {
      mouseSpeed: o.input.mouseSpeed,
      cameraLock: o.input.cameraLock,
      autoAttack: o.input.autoAttack,
      zoomLimit: o.input.zoomLimit,
      offTarget: o.input.offTarget,
      offPets: o.render.hidePets,
    };
  };
}

export function selfOf(ctx: ClientContext): InputDeps['self'] {
  const out: [number, number, number] = [0, 0, 0];
  return (): SelfInfo | null => {
    const rec = ctx.entities.self();
    if (!rec) return null;
    const s = ctx.stores.selfStore.getState();
    const view = viewOf(ctx);
    const vendor = inputProvidersOf(ctx).vendor;
    let dead = false;
    let flying = s.isFlying;
    let pos: Vec3LH = s.pos;
    let angle = 0;
    let state = 0;
    let radius = 0;
    let sitting = false;
    let combat = false;
    const i = view?.indexOf(rec.objId) ?? -1;
    if (view && i >= 0) {
      const flags = view.moversU32[i * STRIDE + MV.flags] ?? 0;
      dead = (flags & FLAGS.DEAD) !== 0;
      flying = (flags & FLAGS.FLY) !== 0;
      if (view.posOf(rec.objId, out)) pos = [out[0], out[1], out[2]];
      const a = view.angleOf(rec.objId);
      angle = Number.isNaN(a) ? 0 : a;
      state = view.moversU32[i * STRIDE + MV.state] ?? 0;
      radius = view.movers[i * STRIDE + MV.radius] ?? 0;
      const stateFlags = view.moversU32[i * STRIDE + MV.stateFlags] ?? 0;
      sitting = (stateFlags & STAF_SIT) !== 0;
      combat = (stateFlags & STAF_COMBAT) !== 0;
    } else if (s.maxHp > 0 && s.hp <= 0) {
      dead = true;
    }
    return {
      id: rec.objId,
      level: s.level,
      dead,
      flying,
      pos,
      angle,
      modeFlags: s.modeFlags,
      state,
      authority: s.authority,
      radius,
      ownVendorOpen: vendor?.ownOpen() ?? false,
      visitingVendor: vendor?.otherOpen() ?? false,
      regionSafe: ctx.stores.worldStore.getState().region.safe,
      settingStone: false,
      sitting,
      combat,
      mp: s.mp,
      chaotic: s.pk.propensity > 0,
      guildId: rec.visual?.guild?.idGuild ?? 0,
    };
  };
}

/** The item in the active hand (`GetActiveHandItemProp`): the right-hand weapon of the own mover's equipment. */
function handOf(ctx: ClientContext): () => HandItem | null {
  return () => {
    const weapon = ctx.entities.self()?.visual?.equip.get(PARTS_RWEAPON);
    const prop = weapon ? ctx.data?.item(weapon.itemId) : undefined;
    if (!prop) return null;
    return {
      id: prop.id,
      kind2: prop.item_kind2,
      kind3: prop.item_kind3,
      attackRange: prop.attack_range,
      useMotion: prop.use_motion,
    };
  };
}

function subjectOf(ctx: ClientContext): InputDeps['cameraSubject'] {
  return (v): CameraSubject | null => {
    const view = v as RenderViewReader | null;
    const rec = ctx.entities.self();
    if (!rec || !view?.valid) return null;
    const i = view.indexOf(rec.objId);
    if (i < 0) return null;
    const flags = view.moversU32[i * STRIDE + MV.flags] ?? 0;
    const sf = view.moversU32[i * STRIDE + MV.stateFlags] ?? 0;
    const at = i * STRIDE + MV.pos;
    return {
      id: rec.objId,
      pos: [view.movers[at] ?? 0, view.movers[at + 1] ?? 0, view.movers[at + 2] ?? 0],
      angle: view.movers[i * STRIDE + MV.angle] ?? 0,
      angleX: view.report[REPORT_ANGLE_X] ?? 0,
      flying: (flags & FLAGS.FLY) !== 0,
      acc: (sf & STAF_ACC) !== 0,
      turbo: (sf & STAF_TURBO) !== 0,
    };
  };
}

export function entityInfoOf(ctx: ClientContext): InputDeps['entityInfo'] {
  const out: [number, number, number] = [0, 0, 0];
  return (id): ObjectInfo | null => {
    const rec = ctx.entities.get(id);
    // a local system pet is neither pickable nor targetable: the server does not know it (WP-CLIENT-31)
    if (!rec || rec.kind === EntityKind.LocalPet) return null;
    const view = viewOf(ctx);
    const known = view?.posOf(id, out) ?? false;
    const idx = view?.indexOf(id) ?? -1;
    const flags = view && idx >= 0 ? (view.moversU32[idx * STRIDE + MV.flags] ?? 0) : 0;
    const kind: ObjectInfo['kind'] =
      rec.kind === EntityKind.Item ? 'item' : rec.kind === EntityKind.Ctrl ? 'ctrl' : 'mover';
    const mode = rec.visual?.mode ?? 0;
    // the MoverProp of an NPC (monsters, guards, merchants); players have none
    const prop = rec.kind === EntityKind.Player ? undefined : ctx.data?.mover(rec.propId);
    // ---- housing / guild house (ui-house-quiz): the `CK1_*` of a ctrl (`propCtrl.txt`)
    const ctrlKind1 =
      rec.kind === EntityKind.Ctrl ? ctx.data?.ctrl(rec.propId)?.ctrl_kind1 : undefined;
    return {
      kind,
      isPlayer: rec.kind === EntityKind.Player,
      peaceful: rec.visual?.peaceful ?? false,
      housing: ctrlKind1 === CK1_HOUSING,
      guildHouse: ctrlKind1 === CK1_GUILD_HOUSE,
      dead: (flags & FLAGS.DEAD) !== 0,
      transparent: (mode & MODE_TRANSPARENT) !== 0,
      vendorOpen: rec.kind === EntityKind.Player && rec.visual?.vendorTitle != null,
      pos: known ? [out[0], out[1], out[2]] : null,
      radius: view && idx >= 0 ? (view.movers[idx * STRIDE + MV.radius] ?? 0) : 0,
      guildId: rec.visual?.guild?.idGuild ?? 0,
      ...(prop
        ? {
            killable: prop.killable !== 0,
            rank: prop.class,
            chaotic: prop.chaotic,
            flyingNpc: prop.flying === 1,
          }
        : {}),
    };
  };
}

function featuresOf(ctx: ClientContext): InputDeps['features'] {
  return (): InputFeatures | null => {
    const f = (ctx.features as { features?: Partial<InputFeatures> } | undefined)?.features;
    return f ?? null;
  };
}

/** The Pointer Lock of the world canvas: a refusal (no gesture, the cool-down after Escape, headless) arrives as `pointerlockerror`. */
function pointerLockOf(canvas: HTMLCanvasElement): PointerLockPort {
  return {
    request: () => {
      try {
        // newer engines return a promise that rejects on a refusal (the `pointerlockerror` event reports it as well)
        const r = canvas.requestPointerLock() as unknown;
        if (r instanceof Promise) r.catch(() => undefined);
      } catch {
        // an environment that throws: the fallback steers
      }
    },
    exit: () => {
      try {
        if (canvas.ownerDocument.pointerLockElement === canvas)
          canvas.ownerDocument.exitPointerLock();
      } catch {
        // already released
      }
    },
  };
}

/** Builds the input layer, attaches it and installs it as `ctx.input`. */
export function installInputHost(ctx: ClientContext, options: InputHostOptions): InputHost {
  const { canvas } = options;
  const win = options.win ?? window;
  const optionsStore = defineOptionsStore(ctx.stores.registry);
  let boundCursor: object | null = null;
  const hooks: InputHooks = {
    toggleFps: () => {
      setOptionByPath(optionsStore, 'input.showFps', !optionsStore.getState().input.showFps);
    },
    // ---- housing / guild house (ui-house-quiz, 45 WP-CFEAT-46): the placement controller answers at the moment of use; inert without the provider
    deployActive: () => inputProvidersOf(ctx).deploy?.active() ?? false,
    deploy: {
      active: () => inputProvidersOf(ctx).deploy?.active() ?? false,
      cursor: (p) => {
        inputProvidersOf(ctx).deploy?.cursor(p);
      },
      confirm: () => {
        inputProvidersOf(ctx).deploy?.confirm();
      },
      cancel: () => inputProvidersOf(ctx).deploy?.cancel() ?? false,
      wheel: (n) => {
        inputProvidersOf(ctx).deploy?.wheel(n);
      },
      pageKey: (code) => {
        inputProvidersOf(ctx).deploy?.pageKey(code);
      },
    },
    // the furniture menu of a clicked guild-house ctrl at the top-right corner of its bound rect; the selection of the row on the focus; Esc clears it
    showCtrlMenu: (id) => {
      inputProvidersOf(ctx).ctrlMenu?.(id, ctx.render?.projectBounds?.(id) ?? null);
    },
    guildFurniture: (id) => {
      const p = inputProvidersOf(ctx).guildFurniture;
      p?.select(id);
      return p !== undefined;
    },
    clearFurnitureSelection: () => {
      inputProvidersOf(ctx).guildFurniture?.clear();
    },
    // Esc step 6 and the focus change read the quest-NPC destination of the quest feature (45 WP-CFEAT-42), absent = inert (PD-01)
    questDestination: {
      active: () => inputProvidersOf(ctx).questDestination?.active() ?? false,
      clear: () => {
        inputProvidersOf(ctx).questDestination?.clear();
      },
    },
    // the revival window (45 WP-CFEAT-12) and the stat-point confirm (42: Enter = Yes, Esc = close) are read at the moment of use
    // (without the revival feature the window counts as open: the Esc chain has nothing to open)
    revival: {
      isOpen: () => inputProvidersOf(ctx).revival?.isOpen() ?? true,
      open: () => {
        inputProvidersOf(ctx).revival?.open();
      },
    },
    statConfirm: {
      yes: () => {
        fullUi(ctx)?.char.statConfirm.yes();
      },
      cancel: () => {
        fullUi(ctx)?.char.statConfirm.cancel();
      },
    },
    // T26: a click on a mover other than the local player plays its idle sound (Plan/44 section 9.3); `ctx.audio` is the
    // engine of boot step B5, read at the moment of the click
    playIdleSound: (id) => {
      ctx.audio.cues?.moverClicked(id);
    },
    guildWarMap: () => ctx.sim.worldId === WI_WORLD_GUILDWAR,
    // 45 WP-CFEAT-45 (M7): the 1:1 guild combat answers at the moment of use; inert (`false`) until its feature sets the provider (43 §4.4, §8.4)
    gc1to1: {
      participant: () => inputProvidersOf(ctx).gc1to1?.participant() ?? false,
      possible: (id) => inputProvidersOf(ctx).gc1to1?.possible(id) ?? false,
    },
    // 45 WP-CFEAT-13: the create-monster item waits for a ground click (43 §7.13, `input/placement.ts` decides, the router sends)
    createMonster: {
      active: () => inputProvidersOf(ctx).createMonster?.active() ?? false,
      leave: () => {
        inputProvidersOf(ctx).createMonster?.leave();
      },
      place: (pos) => inputProvidersOf(ctx).createMonster?.place(pos) ?? false,
    },
    // 45 WP-CFEAT-22: the click on the open shop of another player in a safe region ignores Ctrl
    regionSafe: (id) => inputProvidersOf(ctx).regionSafe?.(id) ?? false,
    // 45 WP-CFEAT-34 / -30: the HUD buff icons (the strip window of 42 claims its icons itself and `uiClaimed` below keeps that click
    // away from the world; this serves a click that reaches the world, and the running client provides no such hook)
    buffIconAt: (x, y) => inputProvidersOf(ctx).buffIconAt?.(x, y) ?? null,
    partySummon: () => {
      inputProvidersOf(ctx).partySummon?.();
    },
    // 45 §11.1 (WP-CFEAT-18): the menu feature resolves the click on a player or an NPC at the moment of use
    showMoverMenu: (id, anchor) => {
      inputProvidersOf(ctx).moverMenu?.(id, anchor);
    },
    // the world-map overlay (45 WP-CFEAT-28): Esc closes it, the clicks of the 3-D view go to it while it is shown
    worldMap: {
      shown: () => inputProvidersOf(ctx).worldMap?.shown() ?? false,
      hide: () => {
        inputProvidersOf(ctx).worldMap?.hide();
      },
      click: (x, y) => {
        inputProvidersOf(ctx).worldMap?.click?.(x, y);
      },
    },
    // the player branch of `IsAttackAble` (45 WP-CFEAT-36: a duel partner is attackable once the fight began)
    playerAttackable: (id) => inputProvidersOf(ctx).playerAttackable?.(id) ?? 0,
    // the wand shot of power >= 1 adds one action point to the display (the server's `SetActionPoint` is the truth)
    actionPointUp: () => {
      skillsRuntime(ctx).actionPointUp();
    },
    // T40 at the melee request site (`WndWorldControlPlayer.cpp:814`); the magic, range and skill starts are the audio layer's own
    // `CmdArrived` subscription
    localAttackStarted: () => {
      ctx.audio.cues?.localAttackStarted();
    },
  };
  const deps: InputDeps = {
    now: () => ctx.now(),
    // BFIX1 #004: the press, move and wheel that the window manager consumed (marked by `UiRoot`) never reach the world
    uiClaimed: (e) => isUiClaimed(e),
    mode: () => {
      const s = ctx.machine.screen.s;
      if (s === 'InGame') return 'game';
      if (
        s === 'Login' ||
        s === 'Connecting' ||
        s === 'CharSelect' ||
        s === 'CharCreate' ||
        s === 'Entering'
      ) {
        return 'title';
      }
      return 'none';
    },
    sim: {
      get cmd() {
        return ctx.sim.cmd;
      },
      rayCast: (o, d, t, m) => ctx.sim.rayCast(o, d, t, m),
      localAct: (msg, p) => ctx.sim.localAct(msg, p),
      queryBudget: () => ctx.sim.queryBudget(),
      heightAttr: (x, z) => ctx.sim.heightAttr(x, z),
      blinkPoint: (s, t, r) => ctx.sim.blinkPoint(s, t, r),
      targetCheck: (t, r, c) => ctx.sim.targetCheck(t, r, c),
      cameraCheckBound: (e, l) => ctx.sim.cameraCheckBound(e, l),
      get worldId() {
        return ctx.sim.worldId;
      },
    },
    render: () => ctx.render,
    windows: windowsOf(ctx),
    taskbar: taskbarOf(ctx),
    chat: chatOf(ctx),
    setWorldCursor: (id) => {
      const cursor = fullUi(ctx)?.cursor;
      if (!cursor) return;
      if (boundCursor !== cursor) {
        // the 3-D canvas carries the world cursor (`ui/NOTES.md` "Mounting"); the UI is created at boot step B5, after this host
        cursor.setWorldTarget(canvas);
        boundCursor = cursor;
      }
      cursor.setWorldCursor(id);
    },
    options: optionsOf(ctx),
    features: featuresOf(ctx),
    self: selfOf(ctx),
    cameraSubject: subjectOf(ctx),
    entityInfo: entityInfoOf(ctx),
    viewport: () => ({
      w: canvas.clientWidth || (typeof window === 'object' ? window.innerWidth : 0),
      h: canvas.clientHeight || (typeof window === 'object' ? window.innerHeight : 0),
    }),
    text: (tid) => ctx.data?.text(tid) ?? '',
    textColor: (tid) => ctx.data?.textColor(tid) ?? 0xffffffff,
    notice: (text, color, style) => {
      fullUi(ctx)?.chat.putString(text, null, color, style);
    },
    hidden: () => ctx.background.hidden,
    // the flight steering lock (D7): requested in the gesture of the flight's R button, released by the flight (Plan/43 section 9.3)
    ...(typeof canvas.requestPointerLock === 'function'
      ? { pointerLock: pointerLockOf(canvas) }
      : {}),
    hand: handOf(ctx),
    // the job-skill table and the action slot (WP-INPUT-13, -14): the skill runtime of 17 and the queue row of the taskbar
    skills: skillsRuntime(ctx).access,
    actionSlot: {
      queue: () => {
        const q = fullUi(ctx)?.taskbar.model.get().queue ?? [];
        return q.filter((sc) => sc !== null).map((sc) => sc.id);
      },
      executing: () => skillsRuntime(ctx).actionSlot.executing,
      setExecuting: (n) => {
        skillsRuntime(ctx).setExecuting(n);
      },
    },
    // the arrow slot belongs to the inventory (45); until it registers its answer every bow has arrows
    bulletEquipped: () => inputProvidersOf(ctx).bulletEquipped?.() ?? true,
    send: (msg) => {
      ctx.net.sender.send(msg);
    },
    hooks,
  };
  const input = createInput(deps);
  input.attach(win, canvas);
  ctx.input = input;

  const offs: (() => void)[] = [];
  // (through the loop, not `ctx.sim.events`: the bridge is installed at boot step B3, after this host)
  offs.push(
    ctx.loop.onSimEvents((events) => {
      for (const raw of events) {
        const e = raw as InputSimEvent;
        if (SIM_EVENTS.has(e.name)) input.onSimEvent(e);
      }
    }),
  );
  // world entry resets auto-run, the selection and the camera; leaving the world drops them
  offs.push(
    ctx.machine.subscribe((screen, previous) => {
      if (screen.s === 'InGame' && previous.s !== 'InGame') input.onWorldEntered();
      else if (previous.s === 'InGame' && screen.s !== 'InGame' && screen.s !== 'Reconnecting') {
        input.onWorldLeft();
      }
    }),
  );
  // the entity of the focus leaves the world (`DelObj`): the focus goes with it
  offs.push(
    ctx.entities.onRemove((rec) => {
      input.core.targeting.onObjectRemoved(rec.objId);
      if (input.core.followId === rec.objId) input.core.followId = null;
    }),
  );
  ctx.probe.extend('input', input.debug);

  return {
    input,
    dispose() {
      for (const off of offs) off();
      input.detach();
    },
  };
}
