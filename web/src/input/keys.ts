// Key state (Plan/43 §3.1): the equivalent of `g_bKeyTable[]` keyed by the physical `KeyboardEvent.code` (D7, PL-127).
//
// Two views of the same keyboard:
//  - `held` is the physical down state, read every tick by the continuous consumers (W/A/S/D, arrows, PgUp/PgDn,
//    Space, X, Z, Tab, Insert);
//  - `pending` is the "table entry set" of the edge-consumed keys (Enter, Esc, applet letters, F-keys, page keys, R):
//    set on EVERY keydown including the auto-repeat, consumed by the tick that acts on it. A key that was released
//    before the tick stays visible for exactly one tick (FIX: the original lost taps shorter than a tick because
//    `WM_KEYUP` cleared the entry before the next `FrameMove`), see {@link KeyState.endTick}.

/** `KeyboardEvent.code`, e.g. `"KeyW"`, `"F1"`, `"NumpadMultiply"`. */
export type KeyCode = string;

/** The modifier state of a key or pointer event. */
export interface Modifiers {
  readonly ctrl: boolean;
  readonly alt: boolean;
  readonly shift: boolean;
  readonly meta: boolean;
}

export const NO_MODIFIERS: Modifiers = { ctrl: false, alt: false, shift: false, meta: false };

const CTRL_CODES: ReadonlySet<KeyCode> = new Set(['ControlLeft', 'ControlRight']);

export class KeyState {
  readonly held = new Set<KeyCode>();
  readonly pending = new Set<KeyCode>();
  /** Keys pressed (a first key-down, not an auto-repeat) since the previous tick. */
  private readonly fresh = new Set<KeyCode>();
  ctrl = false;
  alt = false;
  shift = false;
  meta = false;
  /**
   * `m_bCtrlPushed` (Ctrl+F rule, §3.2): Ctrl was down while a key went down; cleared when Ctrl is released and by
   * {@link KeyState.clear}. (The original cleared it on every key-up; keeping it while Ctrl stays down lets a second
   * Ctrl+F toggle again without releasing Ctrl, which the browser's auto-repeat of Ctrl made unreliable.)
   */
  ctrlPushed = false;
  /** Every `keydown` seen (including repeats): the tests assert the auto-repeat behaviour with it. */
  downEvents = 0;

  setModifiers(m: Modifiers): void {
    this.ctrl = m.ctrl;
    this.alt = m.alt;
    this.shift = m.shift;
    this.meta = m.meta;
  }

  /**
   * A `keydown` routed to the game: the physical state and the edge flag are set (also by an auto-repeat). `edge = false`
   * records the physical state only (a letter or digit action pressed with Ctrl/Alt/Meta down: the modifier gate of
   * §3.2 is decided at the key-down, so a later release of the modifier cannot fire it).
   */
  down(code: KeyCode, m: Modifiers, edge = true): void {
    this.setModifiers(m);
    this.downEvents += 1;
    if (!this.held.has(code)) this.fresh.add(code);
    this.held.add(code);
    if (edge) this.pending.add(code);
    if (CTRL_CODES.has(code) || m.ctrl) this.ctrlPushed = true;
  }

  /** A `keyup`: the physical state is cleared, the edge flag stays until the end of the tick that saw it. */
  up(code: KeyCode, m: Modifiers): void {
    this.setModifiers(m);
    this.held.delete(code);
    if (CTRL_CODES.has(code) && !m.ctrl) this.ctrlPushed = false;
  }

  /** End of a `worldTick`: edge flags of keys that are no longer down are dropped (one tick of visibility). */
  endTick(): void {
    this.fresh.clear();
    for (const code of this.pending) {
      if (!this.held.has(code)) this.pending.delete(code);
    }
  }

  isHeld(code: KeyCode): boolean {
    return this.held.has(code);
  }

  /**
   * The key is down, or went down and up again since the previous tick: the continuous consumers (movement) read this so that
   * a tap shorter than one iteration (a slow frame under load) is not lost, like `pending` keeps an edge key for one tick.
   * A key that was already seen down by an earlier tick stops at once when it is released (no tail). The original lost taps
   * shorter than a tick.
   */
  isDown(code: KeyCode): boolean {
    return this.held.has(code) || this.fresh.has(code);
  }

  anyHeld(codes: readonly KeyCode[]): boolean {
    for (const c of codes) if (this.held.has(c)) return true;
    return false;
  }

  /** `g_bKeyTable[vk] = FALSE` after acting: `true` when the key was pending. */
  consume(code: KeyCode): boolean {
    return this.pending.delete(code);
  }

  isPending(code: KeyCode): boolean {
    return this.pending.has(code);
  }

  /** Window blur or hidden tab (§2.4): no key can stick. */
  clear(): void {
    this.held.clear();
    this.pending.clear();
    this.fresh.clear();
    this.setModifiers(NO_MODIFIERS);
    this.ctrlPushed = false;
  }

  /** A debug copy for the probe. */
  snapshot(): { held: string[]; pending: string[]; ctrl: boolean; alt: boolean; shift: boolean } {
    return {
      held: [...this.held].sort(),
      pending: [...this.pending].sort(),
      ctrl: this.ctrl,
      alt: this.alt,
      shift: this.shift,
    };
  }
}
