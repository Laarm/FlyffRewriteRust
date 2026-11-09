// The keyboard router (Plan/43 §3.5, §3.8, §3.9; WP-INPUT-01): ONE `keydown`/`keyup` listener on `window` (capture phase)
// that decides, for every key, whether it belongs to the game, to a focused edit, to the stat-confirm box, to the chat or to
// nobody, records the game keys in `KeyState` and cancels the browser defaults of the keys it routes to the game.
//
// Rules, in order (R1-R7 of §3.5, the window-side ones are `ui/wm/keyContract.ts`):
//   R1  `isComposing` / `keyCode 229`: the IME owns the key, ignored entirely;
//   R2  a focused edit keeps every key except Escape and F1-F9 (they are cancelled so the input does not see them);
//   R3  title states: the UI's `WBS_KEY` handling (`UiRoot keys`) and the char-select arrow keys own the keyboard;
//       the router only cancels the browser defaults, no world processing;
//   R4  `APP_STATE_CONFIRM` swallows every key (Enter = yes, Escape = close);
//   R5  the logout countdown swallows every key;
//   R6  a modal window swallows every key, Enter focuses the chat over it;
//   R7  otherwise the key is the game's: Enter with the chat open focuses its edit, the key is recorded, and the focused
//       `WBS_KEY` frame sees it.
//
// A `keyup` always clears the physical state (a key released inside an edit must not stay held), but the key-up ACTIONS
// (§3.2 column U) run only when the key was the game's.

import { escapesEdit, killWindowAction, routeKeyDown, type KeyVerdict } from '../ui/wm/keyContract';
import { GATED_CODES, modifierGateOpen, shouldPreventDefault } from './keymap';
import type { KeyCode, KeyState, Modifiers } from './keys';

/** `game`: in the world; `title`: login / character select / create; `none`: nothing of the input layer is active. */
export type InputMode = 'game' | 'title' | 'none';

/** The fields of a `KeyboardEvent` the router reads (a test passes a plain object). */
export interface KeyEventLike {
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly shiftKey: boolean;
  readonly metaKey: boolean;
  readonly isComposing?: boolean;
  readonly keyCode?: number;
  readonly repeat?: boolean;
  preventDefault(): void;
}

/** The window-manager operations of the router (a structural subset of `WindowManager`). */
export interface RouterWindows {
  isEditFocused(): boolean;
  modalOpen(): boolean;
  isStateConfirmOpen(): boolean;
  isLogoutPending(): boolean;
  /** The chat window exists (`GetApplet(APP_COMMUNICATION_CHAT)`). */
  chatOpen(): boolean;
  /** `WBS_KEY` handling of the focused frame; `true` = the key was Enter, Space or Tab of such a frame. */
  keyDown(code: string, info: { shift: boolean; ctrl: boolean; alt: boolean }): boolean;
  keyUp(code: string, info: { shift: boolean; ctrl: boolean; alt: boolean }): boolean;
}

export interface RouterDeps {
  readonly keys: KeyState;
  mode(): InputMode;
  readonly windows: RouterWindows;
  /** Enter: the chat window takes the keyboard (`ChatService.focusInput`, it opens a closed chat and handles a modal). */
  focusChat(): boolean;
  /** `APP_STATE_CONFIRM`: Enter and Escape. */
  readonly statConfirm?: { yes(): void; cancel(): void };
  /** The game side of a recorded key (`CWndWorld::OnKeyDown`: a movement key ends auto-attack). */
  onGameKeyDown?(code: KeyCode, mods: Modifiers): void;
  /** The key-up actions (§3.2 column U); only for keys that were the game's. */
  onGameKeyUp?(code: KeyCode, mods: Modifiers): void;
  /** A focused edit appeared: held movement keys are released (no key may stay down under typing). */
  onEditFocused?(): void;
  /**
   * An `Escape` key-down of the game: `true` = the flight took it (the browser's Pointer Lock exit, or the user leaving the free
   * steering mode) and it neither enters the key state nor runs the Esc chain (Plan/43 §9.3).
   */
  onEscapeDown?(): boolean;
}

/** What the router did with one event (the tests and the debug surface read it). */
export type RouteOutcome =
  | 'ignored'
  | 'edit'
  | 'recorded'
  | 'swallowed'
  | 'killWindow'
  | 'focusChat'
  | 'enterToChat'
  | 'title'
  | 'handledByWindow'
  | 'escapeTaken';

/** The options object form: Node's `EventTarget` does not match a boolean `useCapture` on removal. */
const CAPTURE: AddEventListenerOptions = { capture: true };

const modsOf = (e: KeyEventLike): Modifiers => ({
  ctrl: e.ctrlKey,
  alt: e.altKey,
  shift: e.shiftKey,
  meta: e.metaKey,
});

/** `true` for events the IME owns (R1). */
export function isImeEvent(e: Pick<KeyEventLike, 'isComposing' | 'keyCode'>): boolean {
  return e.isComposing === true || e.keyCode === 229;
}

export class InputRouter {
  /** The last outcome per kind, for the probe. */
  lastDown: { code: string; outcome: RouteOutcome } | null = null;
  lastUp: { code: string; outcome: RouteOutcome } | null = null;
  private wasEditing = false;
  private target: Window | null = null;

  constructor(private readonly deps: RouterDeps) {}

  /** `keydown` (R1-R7). */
  onKeyDown(e: KeyEventLike): RouteOutcome {
    const outcome = this.keyDown(e);
    this.lastDown = { code: e.code, outcome };
    return outcome;
  }

  /** `keyup`. */
  onKeyUp(e: KeyEventLike): RouteOutcome {
    const outcome = this.keyUp(e);
    this.lastUp = { code: e.code, outcome };
    return outcome;
  }

  private cancelDefault(e: KeyEventLike, mods: Modifiers): void {
    if (shouldPreventDefault(e.code, mods)) e.preventDefault();
  }

  private keyDown(e: KeyEventLike): RouteOutcome {
    const d = this.deps;
    if (isImeEvent(e)) return 'ignored'; // R1
    const mode = d.mode();
    if (mode === 'none') return 'ignored';
    const mods = modsOf(e);
    const w = d.windows;
    const editing = w.isEditFocused();
    this.noteEditing(editing);

    if (mode === 'title') {
      // R3: the UI keyboard (`UiRoot keys`) and the char-select window own the key; only the browser defaults are ours.
      // Inside an edit only Escape and F1-F9 are cancelled (the input keeps Tab, Space and Enter for typing).
      if (editing && !escapesEdit(e.code)) return 'edit';
      this.cancelDefault(e, mods);
      return 'title';
    }

    const verdict: KeyVerdict = routeKeyDown(e.code, {
      editFocused: editing,
      modalOpen: w.modalOpen(),
      stateConfirmOpen: w.isStateConfirmOpen(),
      logoutCountdown: w.isLogoutPending(),
      chatOpen: w.chatOpen(),
    });
    switch (verdict) {
      case 'edit':
        return 'edit'; // R2: typing
      case 'killWindow': {
        const action = killWindowAction(e.code);
        if (action === 'yes') d.statConfirm?.yes();
        else if (action === 'close') d.statConfirm?.cancel();
        this.cancelDefault(e, mods);
        return 'killWindow'; // R4
      }
      case 'swallow':
        this.cancelDefault(e, mods);
        return 'swallowed'; // R5, R6
      case 'enterToChat':
        d.focusChat();
        e.preventDefault();
        return 'enterToChat'; // R6 (Enter over a modal)
      case 'focusChat':
        d.focusChat();
        e.preventDefault();
        return 'focusChat'; // R7: the chat edit has the keyboard before the next character arrives
      case 'game':
        break;
    }
    // R7: the game's key
    if (w.keyDown(e.code, { shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt })) {
      e.preventDefault();
      return 'handledByWindow'; // Enter / Space / Tab of a `WBS_KEY` frame
    }
    // the flight takes an Escape that leaves the Pointer Lock or the free steering mode (§9.3)
    if (e.code === 'Escape' && d.onEscapeDown?.() === true) {
      this.cancelDefault(e, mods);
      return 'escapeTaken';
    }
    // movement and camera keys ignore modifiers (a held Ctrl is the PK modifier); letter and digit actions fire only with
    // Ctrl, Alt and Meta up (§3.2, PL-127), decided here at the key-down
    d.keys.down(e.code, mods, !GATED_CODES.has(e.code) || modifierGateOpen(mods));
    d.onGameKeyDown?.(e.code, mods);
    this.cancelDefault(e, mods);
    return 'recorded';
  }

  private keyUp(e: KeyEventLike): RouteOutcome {
    const d = this.deps;
    if (isImeEvent(e)) return 'ignored';
    const mode = d.mode();
    if (mode === 'none') return 'ignored';
    const mods = modsOf(e);
    const w = d.windows;
    // a released key never stays down, whoever had the focus when it went down
    d.keys.up(e.code, mods);
    if (mode === 'title') {
      if (!(w.isEditFocused() && !escapesEdit(e.code))) this.cancelDefault(e, mods);
      return 'title';
    }
    if (w.isEditFocused() && !escapesEdit(e.code)) return 'edit';
    if (w.isLogoutPending() || w.isStateConfirmOpen()) {
      this.cancelDefault(e, mods);
      return 'swallowed';
    }
    if (w.modalOpen()) {
      this.cancelDefault(e, mods);
      return 'swallowed';
    }
    // R7 key-up: window hook first (`WBS_KEY` release), then the world's actions
    const handled = w.keyUp(e.code, { shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt });
    if (!handled) d.onGameKeyUp?.(e.code, mods);
    this.cancelDefault(e, mods);
    return handled ? 'handledByWindow' : 'recorded';
  }

  /** The first key event after an edit got the focus releases whatever was held (a key down before the focus moved). */
  private noteEditing(editing: boolean): void {
    if (editing && !this.wasEditing) {
      this.deps.keys.clear();
      this.deps.onEditFocused?.();
    }
    this.wasEditing = editing;
  }

  /** Window blur / hidden tab (§2.4): nothing stays down. */
  release(): void {
    this.deps.keys.clear();
    this.wasEditing = false;
  }

  /** Installs the listeners on `window` (capture phase). */
  attach(target: Window): () => void {
    this.detach();
    this.target = target;
    const down = (ev: Event): void => {
      this.onKeyDown(ev as KeyboardEvent);
    };
    const up = (ev: Event): void => {
      this.onKeyUp(ev as KeyboardEvent);
    };
    const blur = (): void => {
      this.release();
    };
    target.addEventListener('keydown', down, CAPTURE);
    target.addEventListener('keyup', up, CAPTURE);
    target.addEventListener('blur', blur);
    this.off = () => {
      target.removeEventListener('keydown', down, CAPTURE);
      target.removeEventListener('keyup', up, CAPTURE);
      target.removeEventListener('blur', blur);
    };
    return () => {
      this.detach();
    };
  }

  private off: (() => void) | null = null;

  detach(): void {
    this.off?.();
    this.off = null;
    this.target = null;
  }

  get attached(): boolean {
    return this.target !== null;
  }
}
