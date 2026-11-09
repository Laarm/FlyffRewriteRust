// Pointer plumbing of the world view (Plan/43 §4.1-§4.3, §4.8; WP-INPUT-02): the `WorldMouse` state (the members of
// `CWndWorld`), the Windows double-click detector, and `PointerController`, the DOM glue on the `#world` canvas.
//
// The browser reports a chord (a second button pressed while another is down) as a `pointermove` with a changed `buttons`
// mask, not as a second `pointerdown` (Pointer Events: `pointerdown`/`pointerup` fire only for the first press and the last
// release). The controller therefore diffs the `buttons` mask of EVERY pointer event against its own state and turns each
// changed bit into the button down/up the original saw, so the L+R chord of KQ-31 works.
//
// Pointer capture (`setPointerCapture`) keeps a ground right-drag delivering moves over windows with a visible cursor; Pointer
// Lock is only for flight steering (D7, I5). Touch is out of scope (§12.2): only `mouse` and `pen` pointers are handled.
//
// UI claims (BFIX1 #004): `PointerControllerDeps.uiClaimed(e)` tells whether the UI consumed this very DOM event (the window
// manager marks it, `ui/wm/pointerClaims.ts`). A claimed press, wheel or move never reaches the world: no capture, no click, no zoom,
// no hover (`mouse.inside` goes false, so the per-frame picker clears the highlight). A move is skipped only while the world holds no
// press (a camera drag, a chord or a release over an icon keep syncing); releases, cancels and a lost capture are never gated; under
// the Pointer Lock the claims are ignored (the UI coordinates are frozen and the world owns the pointer).

import type { Modifiers } from './keys';

/** `GetDoubleClickTime()` default. */
export const DBLCLK_MS = 500;
/** `SM_CXDOUBLECLK` default. */
export const DBLCLK_PX = 4;
/** An L-down this soon after an R-down (or the other way round) is a chord: no selection (KQ-31). */
export const LR_SUPPRESS_MS = 200;
/** L and R both down within this long of each other jump (KQ-31). */
export const LR_JUMP_MS = 500;

export type Button = 0 | 1 | 2;

export interface Point {
  readonly x: number;
  readonly y: number;
}

interface LastClick {
  button: Button;
  /** The `BUTTON_BIT` mask of the OTHER buttons that were physically down at the press. */
  held: number;
  x: number;
  y: number;
  t: number;
  valid: boolean;
}

/**
 * The detector of §4.2: the second press of a double click replaces the ordinary press (Windows delivers
 * `WM_xBUTTONDBLCLK` instead of `WM_xBUTTONDOWN`). The click that completes a double click starts no new one, so a triple
 * click is DBLCLK then DOWN.
 *
 * Windows keeps ONE last-click record for all buttons (Wine `clk_msg`: the message and the held-button flags, the time and the
 * position): a press is a double click only when it repeats the record's button with the same other buttons held, and any other
 * press replaces the record. So L,R,L and R,L,R are three ordinary downs, while L,L and R,R stay double clicks (owner #002: with a
 * record per button the second L of "L click, then R+L" became a double click and the second R of "R click, then L+R" an R double
 * click that leaves R logically up, and neither counted as a chord press).
 */
export class DoubleClickDetector {
  private readonly last: LastClick = { button: 0, held: 0, x: 0, y: 0, t: 0, valid: false };

  /**
   * `true` when the press of `button` at `(x, y)` / `t` is the second press of a double click. `held`: the `BUTTON_BIT` mask of the
   * other buttons that are down at the press.
   */
  press(button: Button, x: number, y: number, t: number, held = 0): boolean {
    const l = this.last;
    if (
      l.valid &&
      l.button === button &&
      l.held === held &&
      t - l.t <= DBLCLK_MS &&
      Math.abs(x - l.x) <= DBLCLK_PX / 2 &&
      Math.abs(y - l.y) <= DBLCLK_PX / 2
    ) {
      l.valid = false;
      return true;
    }
    l.button = button;
    l.held = held;
    l.x = x;
    l.y = y;
    l.t = t;
    l.valid = true;
    return false;
  }

  reset(): void {
    this.last.valid = false;
  }
}

/** The mouse members of `CWndWorld` (§4.3). */
export class WorldMouse {
  readonly L = { down: false, downed: false };
  readonly R = { down: false };
  readonly M = { down: false };
  /** `m_timerLButtonDown` / `m_timerRButtonDown`: the ms time of the last L / R press (very old = never). */
  tLDown = Number.NEGATIVE_INFINITY;
  tRDown = Number.NEGATIVE_INFINITY;
  /** `m_ptMouseOld`: the previous pointer position (CSS px relative to the world canvas). */
  ptOld: Point = { x: 0, y: 0 };
  /** `m_ptMouseSpot`: the R-down position (the "no movement while held" test of the player menu). */
  ptSpot: Point = { x: 0, y: 0 };
  /** The current position (unrounded; the pick ray uses it). */
  pos: Point = { x: 0, y: 0 };
  /** The pointer is over the world canvas (not over a window). */
  inside = false;
  /** 1 = flight free steering (R double click while flying), 0 otherwise. */
  mouseMode: 0 | 1 = 0;
  /** The next control step sets a click destination (`m_bFreeMove`). */
  freeMove = false;
  flyMove = false;
  /** Camera drag accumulated since the last camera step (px, before the speed factor). */
  dragDX = 0;
  dragDY = 0;
  /** Flight steering accumulated since the last record (px). */
  steerDX = 0;
  steerDY = 0;
  /** Signed wheel notches since the last camera step (positive = towards the user). */
  wheelNotches = 0;
  readonly dbl = new DoubleClickDetector();

  /** `true` when every button is up. */
  get idle(): boolean {
    return !this.L.down && !this.R.down && !this.M.down;
  }

  /** The state of a window blur / lost capture: no button is down any more (the up handlers do NOT run). */
  releaseAll(): void {
    this.L.down = false;
    this.L.downed = false;
    this.R.down = false;
    this.M.down = false;
    this.freeMove = false;
    this.flyMove = false;
    this.dragDX = 0;
    this.dragDY = 0;
    this.dbl.reset();
  }

  snapshot(): {
    L: boolean;
    R: boolean;
    M: boolean;
    x: number;
    y: number;
    freeMove: boolean;
    mouseMode: number;
    inside: boolean;
  } {
    return {
      L: this.L.down,
      R: this.R.down,
      M: this.M.down,
      x: this.pos.x,
      y: this.pos.y,
      freeMove: this.freeMove,
      mouseMode: this.mouseMode,
      inside: this.inside,
    };
  }
}

/** What a button press carries (the position is CSS px relative to the canvas). */
export interface ButtonEvent {
  readonly button: Button;
  readonly p: Point;
  /** `Math.floor(performance.now())` (the original's `timeGetTime()`). */
  readonly t: number;
  readonly mods: Modifiers;
}

/** The world-click logic of §4.4-§4.8 (WP-INPUT-07) as the controller calls it. */
export interface WorldHandlers {
  /** A press; `dbl` = the second press of a double click (it replaces the ordinary press handler). */
  down(e: ButtonEvent, dbl: boolean): void;
  up(e: ButtonEvent): void;
  /** A move; `movement` is the `movementX/Y` of the event while the Pointer Lock holds (the position is frozen then). */
  move(p: Point, mods: Modifiers, movement?: Point): void;
  /** Normalised wheel notches over the world (positive = towards the user). */
  wheel(notches: number, p: Point, mods: Modifiers): void;
  /** The pointer left the world or the capture was lost: no handler logic, only state. */
  lostCapture(): void;
}

/** The fields of a `PointerEvent` the controller reads (a test passes a plain object). */
export interface PointerEventLike {
  readonly pointerId: number;
  readonly pointerType: string;
  readonly button: number;
  readonly buttons: number;
  readonly clientX: number;
  readonly clientY: number;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly shiftKey: boolean;
  readonly metaKey: boolean;
  /** `PointerEvent.movementX/Y` (CSS pixels): the motion under Pointer Lock (flight steering, WP-INPUT-15). */
  readonly movementX?: number;
  readonly movementY?: number;
  preventDefault(): void;
}

/** The fields of a `WheelEvent` the controller reads. */
export interface WheelLike extends PointerEventLike {
  readonly deltaY: number;
  readonly deltaMode: number;
  readonly wheelDeltaY?: number;
}

/** The canvas operations of the controller (an `HTMLCanvasElement` satisfies it; a test passes an `EventTarget` wrapper). */
export type CanvasLike = Pick<
  HTMLCanvasElement,
  | 'getBoundingClientRect'
  | 'setPointerCapture'
  | 'releasePointerCapture'
  | 'addEventListener'
  | 'removeEventListener'
>;

/** Bit of `PointerEvent.buttons` per button. */
const BUTTON_BIT: readonly [number, number, number] = [1, 4, 2];

function setLogical(m: WorldMouse, b: Button, down: boolean): void {
  if (b === 0) m.L.down = down;
  else if (b === 1) m.M.down = down;
  else m.R.down = down;
}

const mods = (e: PointerEventLike): Modifiers => ({
  ctrl: e.ctrlKey,
  alt: e.altKey,
  shift: e.shiftKey,
  meta: e.metaKey,
});

export interface PointerControllerDeps {
  readonly mouse: WorldMouse;
  readonly handlers: WorldHandlers;
  readonly now: () => number;
  /** Wheel normalisation (see `wheel.ts`), injected so that the tests control the clock. */
  readonly wheelNotches: (e: WheelLike, t: number) => number;
  /** Pointer events are the world's only in the in-world state (§4.1). */
  readonly active: () => boolean;
  /** The release rule of §4.1 needs the mouse mode. */
  readonly onInside?: (inside: boolean) => void;
  /** The world canvas holds the Pointer Lock: moves carry `movementX/Y` instead of a position (flight steering, §9.3). */
  readonly locked?: () => boolean;
  /**
   * The UI consumed this DOM event (a window or an icon of a pass-through window was under the pointer, a modal is open): the world
   * must not act on a press, a wheel or a move that is claimed. The event object is the one the canvas listener received.
   */
  readonly uiClaimed?: (e: PointerEventLike) => boolean;
}

export class PointerController {
  private canvas: CanvasLike | null = null;
  private captured = false;
  /** The physical buttons (L, M, R): `WorldMouse` holds the LOGICAL state (an R double click leaves R logically up). */
  private readonly held: [boolean, boolean, boolean] = [false, false, false];
  private readonly off: (() => void)[] = [];

  constructor(private readonly d: PointerControllerDeps) {}

  /** The canvas holds the pointer capture of a press (the debug surface and the tests read it). */
  get isCaptured(): boolean {
    return this.captured;
  }

  /** Installs the listeners on the world canvas. */
  attach(canvas: CanvasLike): void {
    this.detach();
    this.canvas = canvas;
    const add = (
      type: string,
      fn: (e: WheelLike) => void,
      options?: AddEventListenerOptions,
    ): void => {
      const listener = (e: Event): void => {
        fn(e as unknown as WheelLike);
      };
      canvas.addEventListener(type, listener, options);
      this.off.push(() => {
        canvas.removeEventListener(type, listener, options);
      });
    };
    add('pointerdown', (e) => {
      this.onPointerDown(e);
    });
    add('pointermove', (e) => {
      this.onPointerMove(e);
    });
    add('pointerup', (e) => {
      this.onPointerUp(e);
    });
    add('pointercancel', () => {
      this.onPointerCancel();
    });
    add('lostpointercapture', () => {
      this.onLostCapture();
    });
    add('pointerleave', () => {
      this.setInside(false);
    });
    add('pointerenter', () => {
      this.setInside(true);
    });
    add(
      'wheel',
      (e) => {
        this.onWheel(e);
      },
      { passive: false },
    );
    // §3.8: the browser defaults of the world surface
    add('contextmenu', (e) => {
      e.preventDefault();
    });
    add('auxclick', (e) => {
      e.preventDefault();
    });
    add('mousedown', (e) => {
      // middle: Windows auto-scroll; buttons 3/4 (back/forward): history navigation
      if (e.button === 1 || e.button >= 3) e.preventDefault();
    });
    add('mouseup', (e) => {
      if (e.button >= 3) e.preventDefault();
    });
    add('dragstart', (e) => {
      e.preventDefault();
    });
    add('selectstart', (e) => {
      e.preventDefault();
    });
  }

  detach(): void {
    for (const f of this.off.splice(0)) f();
    this.canvas = null;
    this.captured = false;
  }

  private setInside(v: boolean): void {
    if (this.d.mouse.inside === v) return;
    this.d.mouse.inside = v;
    this.d.onInside?.(v);
  }

  private posOf(e: PointerEventLike): Point {
    const r = this.canvas?.getBoundingClientRect() ?? { left: 0, top: 0 };
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private handled(e: PointerEventLike): boolean {
    return (e.pointerType === 'mouse' || e.pointerType === 'pen') && this.d.active();
  }

  /** The UI claimed this event and the world has no say in it (never under the Pointer Lock). */
  private claimed(e: PointerEventLike): boolean {
    return this.d.uiClaimed?.(e) === true && this.d.locked?.() !== true;
  }

  /** The world has a press in progress (it holds the capture or a physical button went down on it). */
  private get worldPress(): boolean {
    return this.captured || this.held[0] || this.held[1] || this.held[2];
  }

  /** The first press: capture the pointer, then the button diff. */
  onPointerDown(e: PointerEventLike): void {
    if (!this.handled(e)) return;
    if (e.button >= 3) {
      e.preventDefault(); // back/forward
      return;
    }
    if (this.claimed(e)) {
      // a window or an icon took the press: no capture, no world press, no hover behind it
      this.setInside(false);
      return;
    }
    this.setInside(true);
    try {
      this.canvas?.setPointerCapture(e.pointerId);
      this.captured = true;
    } catch {
      // a pointer that is no longer active: the press still counts, only the capture is missing
    }
    this.sync(e);
  }

  /** A move, or a chord: the `buttons` mask tells which buttons changed. */
  onPointerMove(e: PointerEventLike): void {
    if (!this.handled(e)) return;
    if (this.claimed(e) && !this.worldPress) {
      // over a window or an icon with no world press: the world sees nothing (the per-frame picker clears its highlight)
      this.setInside(false);
      return;
    }
    this.setInside(true);
    // a button that went down on a WINDOW and dragged onto the world is not the world's press: only a press this controller saw
    // (it holds the capture) can have a chord partner or a release in a move event
    if (this.worldPress) this.sync(e);
    const p = this.posOf(e);
    this.d.mouse.pos = p;
    // under Pointer Lock the position is frozen: the motion is the event's `movementX/Y`
    if (this.d.locked?.() === true) {
      this.d.handlers.move(p, mods(e), { x: e.movementX ?? 0, y: e.movementY ?? 0 });
    } else {
      this.d.handlers.move(p, mods(e));
    }
  }

  onPointerUp(e: PointerEventLike): void {
    if (!this.handled(e)) {
      if (e.button >= 3) e.preventDefault();
      return;
    }
    if (e.button >= 3) {
      e.preventDefault();
      return;
    }
    this.sync(e);
    this.releaseCapture(e.pointerId);
  }

  onPointerCancel(): void {
    this.onLostCapture();
  }

  /** Alt-tab, devtools, a lost pointer: the up state without the up handlers' game logic (§4.1). */
  onLostCapture(): void {
    const physical = this.held[0] || this.held[1] || this.held[2];
    if (!physical && this.d.mouse.idle) {
      this.captured = false;
      return;
    }
    this.held.fill(false);
    this.captured = false;
    this.d.mouse.releaseAll(); // the up state, WITHOUT the up handlers' game logic
    this.d.handlers.lostCapture();
  }

  private releaseCapture(id: number): void {
    const m = this.d.mouse;
    // L-up releases when mouseMode == 0 and R is up; R-up and M-up release when mouseMode == 0 (§4.1)
    if (m.mouseMode !== 0 || !m.idle) return;
    try {
      this.canvas?.releasePointerCapture(id);
    } catch {
      // already released by the browser
    }
    this.captured = false;
  }

  /**
   * Turns the changed bits of `e.buttons` into the downs and ups of the original. The logical button state of
   * `WorldMouse` is set before the down handler runs and cleared before the up handler runs (the original cleared it AFTER
   * `OnXButtonUp`, `WndBase.cpp:1104` vs `:1142-1156`; harmless here, because the L+R chord is checked at the press, owner #002);
   * the second press of an R double click does NOT set R (no camera drag on it, §4.2).
   */
  private sync(e: PointerEventLike): void {
    const m = this.d.mouse;
    const p = this.posOf(e);
    m.pos = p;
    const t = this.d.now();
    const md = mods(e);
    // releases first, so that a release and a press in one event keep the order of the buttons
    for (const b of [0, 1, 2] as const) {
      const is = (e.buttons & BUTTON_BIT[b]) !== 0;
      if (this.held[b] && !is) {
        this.held[b] = false;
        setLogical(m, b, false);
        this.d.handlers.up({ button: b, p, t, mods: md });
      }
    }
    for (const b of [0, 1, 2] as const) {
      const is = (e.buttons & BUTTON_BIT[b]) !== 0;
      if (!this.held[b] && is) {
        this.held[b] = true;
        const dbl = m.dbl.press(b, p.x, p.y, t, this.heldOthers(b));
        if (!(b === 2 && dbl)) setLogical(m, b, true);
        this.d.handlers.down({ button: b, p, t, mods: md }, dbl);
      }
    }
  }

  /** The `BUTTON_BIT` mask of the physical buttons down other than `b` (the double-click record's `held`). */
  private heldOthers(b: Button): number {
    let mask = 0;
    for (const o of [0, 1, 2] as const) {
      if (o !== b && this.held[o]) mask |= BUTTON_BIT[o];
    }
    return mask;
  }

  private onWheel(e: WheelLike): void {
    if (!this.d.active()) return;
    e.preventDefault();
    if (this.claimed(e)) return; // the window under the pointer got the wheel: the camera does not zoom
    const n = this.d.wheelNotches(e, this.d.now());
    if (n === 0) return;
    this.d.handlers.wheel(n, this.posOf(e), mods(e));
  }
}
