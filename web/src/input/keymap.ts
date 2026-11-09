// The key map (Plan/43 §3.2, §3.3; ref/21 §8.3, ref/22 §5.3, `FuncApplet.cpp:162-310`): data, not code, so that a later
// options page can rebind keys (P2, §12.3; v1 reads no keymap). Keys are physical `KeyboardEvent.code`s (D7, I1): the
// defaults are the English layout of the original and keep their physical positions on every other layout (PL-127).
//
// Context: `G` ground world control, `F` flying, `W` any in-world state, `T` title states.
// Trigger: `held` read every tick, `edge` consumed by the tick that acts on it (`pending`), `up` on key-up,
// `rising` rising edge against the previous tick (X, Space and Z while flying, Tab while flying).

import {
  APP_BAG_EX,
  APP_CHARACTER3,
  APP_COMM_ITEM,
  APP_COUPLE_MAIN,
  APP_FTOOL,
  APP_GUILD,
  APP_INVENTORY,
  APP_LORD_SKILL,
  APP_MAP,
  APP_MESSENGER_EX,
  APP_MOTION,
  APP_NAVIGATOR,
  APP_PARTY,
  APP_PARTY_FIND,
  APP_PETFILTER,
  APP_QUEST_EX_LIST,
  APP_REFRESHER,
  APP_SKILL3,
  APP_TELE16,
} from '../gen/defs/resdata';
import type { ClientFeatures } from '../gen/config/clientFeatures';
import type { KeyCode, Modifiers } from './keys';

export type KeyContext = 'G' | 'F' | 'W' | 'T';
export type KeyTrigger = 'held' | 'edge' | 'up' | 'rising';

/** What a key does (the `Action` column of §3.2). */
export const KeyAction = {
  Forward: 'forward',
  Backward: 'backward',
  TurnLeft: 'turnLeft',
  TurnRight: 'turnRight',
  Jump: 'jump',
  WalkToggle: 'walkToggle',
  Follow: 'follow',
  ActionSlot: 'actionSlot',
  WhisperReply: 'whisperReply',
  TargetCycle: 'targetCycle',
  Chat: 'chat',
  Escape: 'escape',
  ItemSlot: 'itemSlot',
  ItemPage: 'itemPage',
  CamLeft: 'camLeft',
  CamRight: 'camRight',
  CamUp: 'camUp',
  CamDown: 'camDown',
  ZoomIn: 'zoomIn',
  ZoomOut: 'zoomOut',
  Telescope: 'telescope',
  ToggleFps: 'toggleFps',
  HideWindows: 'hideWindows',
  FlyAttack: 'flyAttack',
  CharLeft: 'charLeft',
  CharRight: 'charRight',
} as const;
export type KeyAction = (typeof KeyAction)[keyof typeof KeyAction];

export interface KeyBinding {
  readonly code: KeyCode;
  readonly action: KeyAction;
  readonly ctx: KeyContext;
  readonly trigger: KeyTrigger;
  /** The modifier rule of §3.2: fires only while Ctrl, Alt and Meta are all up (Shift is allowed). */
  readonly gated: boolean;
  /** `ItemSlot` 0..8, `ItemPage` 0..7. */
  readonly index?: number;
}

function b(
  code: KeyCode,
  action: KeyAction,
  ctx: KeyContext,
  trigger: KeyTrigger,
  gated = false,
  index?: number,
): KeyBinding {
  return index === undefined
    ? { code, action, ctx, trigger, gated }
    : { code, action, ctx, trigger, gated, index };
}

const FKEYS: readonly KeyBinding[] = Array.from({ length: 9 }, (_, i) =>
  b(`F${String(i + 1)}`, KeyAction.ItemSlot, 'W', 'edge', false, i),
);
const DIGITS: readonly KeyBinding[] = Array.from({ length: 8 }, (_, i) =>
  b(`Digit${String(i + 1)}`, KeyAction.ItemPage, 'W', 'edge', true, i),
);

/**
 * `KEYMAP` (§3.2). `Digit0` (screenshot) and `Digit9` (position dump) are DROPPED (F-350, PL-163) and F10 is not a game
 * key (PL-124): none of them has a row.
 */
export const KEYMAP: readonly KeyBinding[] = [
  b('KeyW', KeyAction.Forward, 'G', 'held'),
  b('KeyS', KeyAction.Backward, 'G', 'held'),
  b('KeyA', KeyAction.TurnLeft, 'G', 'held'),
  b('KeyD', KeyAction.TurnRight, 'G', 'held'),
  b('Space', KeyAction.Jump, 'G', 'held'),
  b('KeyX', KeyAction.WalkToggle, 'G', 'rising'),
  b('KeyZ', KeyAction.Follow, 'G', 'held', true),
  b('KeyC', KeyAction.ActionSlot, 'G', 'up', true),
  b('KeyR', KeyAction.WhisperReply, 'G', 'edge', true),
  b('Tab', KeyAction.TargetCycle, 'G', 'up'),
  b('Enter', KeyAction.Chat, 'W', 'edge'),
  b('NumpadEnter', KeyAction.Chat, 'W', 'edge'),
  b('Escape', KeyAction.Escape, 'W', 'edge'),
  ...FKEYS,
  ...DIGITS,
  b('ArrowLeft', KeyAction.CamLeft, 'W', 'held'),
  b('ArrowRight', KeyAction.CamRight, 'W', 'held'),
  b('ArrowUp', KeyAction.CamUp, 'W', 'held'),
  b('ArrowDown', KeyAction.CamDown, 'W', 'held'),
  b('PageUp', KeyAction.ZoomIn, 'W', 'held'),
  b('PageDown', KeyAction.ZoomOut, 'W', 'held'),
  b('ScrollLock', KeyAction.Telescope, 'W', 'up'),
  b('KeyF', KeyAction.ToggleFps, 'W', 'up'),
  b('NumpadMultiply', KeyAction.HideWindows, 'W', 'up'),
  b('Insert', KeyAction.FlyAttack, 'F', 'held'),
  b('ArrowLeft', KeyAction.CharLeft, 'T', 'up'),
  b('ArrowRight', KeyAction.CharRight, 'T', 'up'),
];

/** Every code that is bound to `action` in the context `ctx` (`W` rows count for every in-world context). */
export function codesOf(action: KeyAction, ctx?: KeyContext): readonly KeyCode[] {
  const out: KeyCode[] = [];
  for (const k of KEYMAP) {
    if (k.action !== action) continue;
    if (ctx !== undefined && k.ctx !== ctx && k.ctx !== 'W') continue;
    out.push(k.code);
  }
  return out;
}

/** The bindings of one code. */
export function bindingsOf(code: KeyCode): readonly KeyBinding[] {
  return KEYMAP.filter((k) => k.code === code);
}

/** `true` when Ctrl, Alt and Meta are all up (Shift is allowed): the gate of letter and digit actions (§3.2, PL-127). */
export function modifierGateOpen(m: Modifiers): boolean {
  return !m.ctrl && !m.alt && !m.meta;
}

/** A feature switch that enables a hotkey (`ClientFeatures` key, and the value it must have). */
export interface FeatureGate {
  readonly feature: keyof ClientFeatures;
  readonly value: boolean;
}

export interface AppletHotkey {
  readonly code: KeyCode;
  /** `APP_*` id of the window (`wm.objectExecutorApplet(app)`). */
  readonly app: number;
  /** The hotkey exists only while the feature has this value. */
  readonly gate?: FeatureGate;
}

/**
 * `APPLET_HOTKEYS` (§3.3, `CWndTaskBar::Process`): an entry fires when its code is pending, the modifier gate is open and
 * its feature gate holds. `KeyT` (F-Tool) and `KeyR` (farm bot refresher) are reserved off (PL-140, PL-141: the whisper
 * reply keeps R); `KeyQ` needs `features.no_quest_button == false`.
 */
export const APPLET_HOTKEYS: readonly AppletHotkey[] = [
  { code: 'KeyN', app: APP_NAVIGATOR },
  { code: 'KeyM', app: APP_MAP },
  { code: 'KeyH', app: APP_CHARACTER3 },
  { code: 'KeyI', app: APP_INVENTORY },
  { code: 'KeyK', app: APP_SKILL3 },
  { code: 'KeyO', app: APP_MOTION },
  { code: 'KeyP', app: APP_PARTY },
  { code: 'KeyG', app: APP_GUILD },
  { code: 'KeyB', app: APP_BAG_EX },
  { code: 'KeyJ', app: APP_COMM_ITEM },
  { code: 'KeyE', app: APP_MESSENGER_EX },
  { code: 'KeyF', app: APP_COUPLE_MAIN },
  { code: 'KeyL', app: APP_LORD_SKILL },
  { code: 'KeyY', app: APP_PETFILTER, gate: { feature: 'pet_filter', value: true } },
  { code: 'KeyU', app: APP_PARTY_FIND, gate: { feature: 'party_find', value: true } },
  { code: 'KeyV', app: APP_TELE16, gate: { feature: 'teleportation_system', value: true } },
  { code: 'KeyT', app: APP_FTOOL, gate: { feature: 'ftool', value: true } },
  { code: 'KeyR', app: APP_REFRESHER, gate: { feature: 'farm_bot', value: true } },
  { code: 'KeyQ', app: APP_QUEST_EX_LIST, gate: { feature: 'no_quest_button', value: false } },
];

/**
 * The codes whose edge actions obey the modifier gate (§3.2): the gated rows of `KEYMAP` and every applet letter. The
 * router records them as edges only while Ctrl, Alt and Meta are up.
 */
export const GATED_CODES: ReadonlySet<KeyCode> = new Set([
  ...KEYMAP.filter((k) => k.gated).map((k) => k.code),
  ...APPLET_HOTKEYS.map((h) => h.code),
]);

/** The tooltip letter of an applet (`"%s\n[%s %c]"`, `TID_GAME_TOOLTIP_HOTKEY`): the US-layout letter, or `null`. */
export function appletHotkeyLetter(app: number): string | null {
  const h = APPLET_HOTKEYS.find((e) => e.app === app);
  return h ? h.code.replace('Key', '') : null;
}

/** An entry is active under `features` (an absent feature table enables only the ungated entries). */
export function appletHotkeyActive(
  h: AppletHotkey,
  features: Readonly<Partial<ClientFeatures>> | null,
): boolean {
  if (!h.gate) return true;
  const v = features?.[h.gate.feature];
  return v === h.gate.value;
}

/**
 * The keys whose `keydown`/`keyup` default is cancelled while the key is routed to the game (§3.8): function keys,
 * focus traversal, scrolling, find-as-you-type, the Windows menu bar and `Escape`.
 */
const PREVENT_CODES: ReadonlySet<KeyCode> = new Set([
  'F1',
  'F2',
  'F3',
  'F4',
  'F5',
  'F6',
  'F7',
  'F8',
  'F9',
  'F10',
  'Tab',
  'Space',
  'Enter',
  'NumpadEnter',
  'Backspace',
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'PageUp',
  'PageDown',
  'Home',
  'End',
  'Escape',
  'Slash',
  'Quote',
  'AltLeft',
  'AltRight',
]);

/**
 * Whether the router calls `preventDefault()` on a key it routes to the game (§3.8). Never cancelled: `F11`, `F12` and every
 * Ctrl/Meta combination other than Ctrl+F (the browser keeps its shortcuts).
 */
export function shouldPreventDefault(code: KeyCode, m: Modifiers): boolean {
  if (code === 'F11' || code === 'F12') return false;
  if (code === 'KeyF' && m.ctrl && !m.meta && !m.alt) return true;
  if (m.ctrl || m.meta) return false;
  return PREVENT_CODES.has(code);
}
