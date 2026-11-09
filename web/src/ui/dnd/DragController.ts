// `DragController` (`ui.dnd`, Plan/42 §10, ref/22 §11; WP-UI-12): the drag-and-drop core.
//
// The payload is a `Shortcut` (the legacy `SHORTCUT`, kinds 0-11). A drag starts on the FIRST pointer move after a press
// on a source (an item cell, a taskbar slot, a start-menu item): the source calls `arm(build)` on its press, the next
// `pointerMove` of the window manager turns the armed source into a drag (`begin`), a release without a move starts
// nothing (a click). While a shortcut is dragged the manager reports `isDraggingShortcut()` (button highlights, hotkeys
// and tooltips are off) and the drag layer draws the icon at the pointer.
//
// A drop is hit-tested by the manager in UI virtual pixels (the same hit test as every pointer event, so the drag layer
// itself is never a target): a control with `onDropIcon` takes it (buttons forward `WIN_ITEMDROP` to their frame), a drop
// on no window goes to the world-drop handler 45 registers (`setWorldDrop`), and a refusal calls `forbid()`: the
// `Icon/icon_forbid.dds` at the pointer for 500 ms (`SetForbid`, `m_timerForbid.Set(500)`). Esc does not cancel a drag
// (the Esc chain checks `isDraggingShortcut`); `pointercancel` ends it without a drop.

import type { Point } from '../layout/geometry';
import type { DragSink, WindowManager, WindowManagerEvent } from '../wm/WindowManager';
import type { WndBase } from '../wnd/WndBase';
import { Observable } from '../wnd/observable';
import type { Shortcut } from './shortcut';

/** `m_timerForbid.Set(500)`. */
export const FORBID_MS = 500;
/** The forbid icon (`Icon/icon_forbid.dds`). */
export const FORBID_ICON = 'icon/icon_forbid.dds';
/** The icon follows the pointer at this offset (`m_pTexture->Render(pt - (16, 16))`). */
export const DRAG_ICON_OFFSET = -16;

/** What the drag layer renders. */
export interface DragView {
  /** The shortcut being dragged, `null` when none. */
  readonly shortcut: Shortcut | null;
  /** The pointer in virtual pixels. */
  readonly pt: Point;
  /** The forbid icon: where it is shown and until when (`host.now()` ms), `null` when hidden. */
  readonly forbid: { readonly pt: Point; readonly until: number } | null;
}

/** How a drop on no window is handled (45's `CWndWorld::OnDropIcon`); `false` = refused (the forbid icon shows). */
export type WorldDropHandler = (sc: Shortcut, pt: Point) => boolean;

/** Told about the end of every drag (the taskbar removes a slot that was dragged out of it). */
export interface DragEnd {
  readonly shortcut: Shortcut;
  /** The window under the pointer at the release, `null` over the world or after a cancel. */
  readonly target: WndBase | null;
  /** A window's `onDropIcon` returned `true`, or the world-drop handler did. */
  readonly accepted: boolean;
  /** `true` when the drop reached the world (no window under the pointer). */
  readonly world: boolean;
  /** `true` after `pointercancel` (no drop happened). */
  readonly cancelled: boolean;
  readonly pt: Point;
}

interface Armed {
  readonly source: WndBase;
  readonly build: () => Shortcut | null;
}

export interface DragControllerDeps {
  readonly wm: WindowManager;
  /** `host.now()` (ms): the forbid icon expires on the UI tick. */
  readonly now: () => number;
}

const ORIGIN: Point = { x: 0, y: 0 };

export class DragController implements DragSink {
  readonly #wm: WindowManager;
  readonly #now: () => number;
  readonly #view = new Observable<DragView>({ shortcut: null, pt: ORIGIN, forbid: null });
  readonly #endListeners = new Set<(e: DragEnd) => void>();
  readonly #claimers = new Set<WorldDropHandler>();
  #armed: Armed | null = null;
  #world: WorldDropHandler | null = null;
  #pt: Point = ORIGIN;
  #off: (() => void) | null = null;
  readonly #dropLog: DragEnd[] = [];

  constructor(deps: DragControllerDeps) {
    this.#wm = deps.wm;
    this.#now = deps.now;
  }

  /** React subscription (the `DragLayer`). */
  readonly subscribe = this.#view.subscribe;
  readonly getSnapshot = this.#view.get;

  /** `CWndBase::m_GlobalShortcut`: the shortcut being dragged, `null` when none. */
  get current(): Shortcut | null {
    return this.#wm.draggedShortcut;
  }

  /** `true` while a shortcut is dragged: letter hotkeys are off (§5.6), buttons do not highlight, tooltips are off. */
  get hotkeysSuppressed(): boolean {
    return this.#wm.isDraggingShortcut();
  }

  /** The forbid icon is on screen. */
  get forbidVisible(): boolean {
    return this.#view.get().forbid !== null;
  }

  /** Hooks the controller to the manager (idempotent); returns the unsubscribe function. */
  attach(): () => void {
    if (this.#off) return this.#off;
    this.#wm.dragSink = this;
    const off = this.#wm.onEvent((e) => {
      this.#onEvent(e);
    });
    this.#off = () => {
      off();
      if (this.#wm.dragSink === this) this.#wm.dragSink = null;
      this.#off = null;
    };
    return this.#off;
  }

  /** 45 registers the handler of drops on the 3-D view (drop to the ground with the quantity dialog, refusals). */
  setWorldDrop(handler: WorldDropHandler | null): void {
    this.#world = handler;
  }

  /**
   * A claimer sees the drops on no window before the world handler does (the taskbar takes a slot that was dragged out
   * of it and deletes the slot); the first that returns `true` accepts the drop and no forbid icon shows.
   */
  addWorldClaimer(fn: WorldDropHandler): () => void {
    this.#claimers.add(fn);
    return () => {
      this.#claimers.delete(fn);
    };
  }

  /** Observes the end of every drag; returns the unsubscribe function. */
  onEnd(fn: (e: DragEnd) => void): () => void {
    this.#endListeners.add(fn);
    return () => {
      this.#endListeners.delete(fn);
    };
  }

  /** The last ends of drags (probe and tests): the newest last, at most 16. */
  recentEnds(): readonly DragEnd[] {
    return this.#dropLog;
  }

  // ---- starting a drag ---------------------------------------------------------------------------------------

  /**
   * A source calls this on its press: the next pointer move turns it into a drag of `build()` (the payload is built at
   * that moment, from the current state of the source; `null` = nothing to drag). A release without a move discards
   * it. Ignored while another shortcut is dragged.
   */
  arm(source: WndBase, build: () => Shortcut | null): void {
    if (this.current) return;
    this.#armed = { source, build };
  }

  /** Discards an armed press (the source was destroyed or refused the drag). */
  disarm(): void {
    this.#armed = null;
  }

  get armed(): boolean {
    return this.#armed !== null;
  }

  /**
   * `begin`: starts dragging `sc` at the current pointer position. Hides a visible forbid icon. Returns `false` while
   * another shortcut is dragged.
   */
  begin(sc: Shortcut): boolean {
    if (this.current) return false;
    this.#armed = null;
    this.#wm.setDraggedShortcut(sc);
    this.#view.set({ shortcut: sc, pt: this.#pt, forbid: null });
    return true;
  }

  /** Ends the drag without a drop (the Esc chain never calls it; `pointercancel` does). */
  cancel(): void {
    const sc = this.current;
    this.#armed = null;
    if (!sc) return;
    this.#wm.setDraggedShortcut(null);
    this.#publishEnd();
    this.#finish({
      shortcut: sc,
      target: null,
      accepted: false,
      world: false,
      cancelled: true,
      pt: this.#pt,
    });
  }

  // ---- refusing ------------------------------------------------------------------------------------------------

  /** `SetForbid(TRUE)`: the forbid icon at the pointer for 500 ms. */
  forbid(): void {
    const v = this.#view.get();
    this.#view.set({
      ...v,
      forbid: { pt: this.#pt, until: this.#now() + FORBID_MS },
    });
  }

  // ---- DragSink (called by the manager) ---------------------------------------------------------------------------

  dropped(sc: Shortcut, target: WndBase | null, accepted: boolean, pt: Point): void {
    this.#armed = null;
    this.#pt = pt;
    this.#publishEnd();
    let ok = accepted;
    const world = target === null;
    if (world) {
      // a drop on no window: the claimers first (the taskbar), then `CWndWorld::OnDropIcon` (45). Without a handler
      // the world refuses everything.
      ok = [...this.#claimers].some((claim) => claim(sc, pt));
      if (!ok) ok = this.#world ? this.#world(sc, pt) : false;
      if (!ok) this.forbid();
    }
    this.#finish({ shortcut: sc, target, accepted: ok, world, cancelled: false, pt });
  }

  cancelled(sc: Shortcut): void {
    this.#armed = null;
    this.#publishEnd();
    this.#finish({
      shortcut: sc,
      target: null,
      accepted: false,
      world: false,
      cancelled: true,
      pt: this.#pt,
    });
  }

  // ---- internals ---------------------------------------------------------------------------------------------

  #publishEnd(): void {
    const v = this.#view.get();
    this.#view.set({ ...v, shortcut: null });
  }

  #finish(e: DragEnd): void {
    this.#dropLog.push(e);
    if (this.#dropLog.length > 16) this.#dropLog.shift();
    for (const fn of [...this.#endListeners]) fn(e);
  }

  #onEvent(e: WindowManagerEvent): void {
    switch (e.type) {
      case 'pointerMove': {
        this.#pt = e.pt;
        const armed = this.#armed;
        if (armed && !this.current) {
          this.#armed = null;
          if (armed.source.destroyed) break;
          const sc = armed.build();
          if (sc) this.begin(sc);
          break;
        }
        const v = this.#view.get();
        if (v.shortcut) this.#view.set({ ...v, pt: e.pt });
        break;
      }
      case 'pointerUp':
        // a release without a move was a click
        this.#armed = null;
        break;
      case 'tick': {
        const f = this.#view.get().forbid;
        if (f && e.now >= f.until) this.#view.set({ ...this.#view.get(), forbid: null });
        break;
      }
      case 'pointerDown':
      case 'opened':
      case 'closed':
      case 'focus':
      case 'hover':
      case 'viewport':
        break;
    }
  }
}
