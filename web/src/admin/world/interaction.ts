// The pointer and the keyboard of the map (Plan/25 section 3.2 "Selection, search, follow and keyboard", 3.9): drag pans (and ends follow), the
// wheel zooms x1.25 at the cursor, a double click x2, click selects, Shift+click adds, B (or the box tool) drags a selection box, the ruler and the
// point-picking mode take clicks, a right click goes to the context-menu listeners. Keys are inactive while an input has the focus.
import type { MapEngine } from './engine';
import type { MapSession } from './session';
import { dragRect } from './picking';
import { DOUBLE_CLICK_STEP, WHEEL_STEP } from './view';

export interface InteractionDeps {
  readonly host: HTMLElement;
  readonly engine: MapEngine;
  readonly session: MapSession;
  /** Focuses the find box (key `/`). */
  readonly focusFind: () => void;
}

/** A plain character typed into something editable: the single-key shortcuts stay off. */
export function typingInto(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) ||
    // a dialog, or a menu that has the focus (the context menu closes on its own Escape)
    target.closest('[role="dialog"], [role="menu"]') !== null
  );
}

/** Pixels a drag must move before it counts as a drag and not a click. */
const DRAG_PX = 4;

type Mode = 'idle' | 'pan' | 'box';

/** Attaches the listeners; returns the remover. */
export function attachInteraction(d: InteractionDeps): () => void {
  const { host, engine, session } = d;
  let mode: Mode = 'idle';
  let pointerId = -1;
  let startX = 0;
  let startY = 0;
  let lastX = 0;
  let lastY = 0;
  let moved = false;
  let startWorld = { x: 0, z: 0 };
  let boxKey = false;
  let cursorPending = false;
  let cursor = { sx: 0, sy: 0 };

  const local = (e: { clientX: number; clientY: number }): { sx: number; sy: number } => {
    const r = host.getBoundingClientRect();
    return { sx: e.clientX - r.left, sy: e.clientY - r.top };
  };

  const publishCursor = (): void => {
    cursorPending = false;
    const w = engine.screenToWorld(cursor.sx, cursor.sy);
    session.store.setState({ cursor: { x: w.x, z: w.z } });
  };

  const onDown = (e: PointerEvent): void => {
    session.touch();
    if (e.button !== 0) return;
    const p = local(e);
    const w = engine.screenToWorld(p.sx, p.sy);
    const tool = session.ui.tool;
    if (tool === 'pick') {
      session.pickAt(w.x, w.z);
      return;
    }
    if (tool === 'ruler') {
      session.rulerPoint(w.x, w.z);
      return;
    }
    host.setPointerCapture(e.pointerId);
    pointerId = e.pointerId;
    startX = lastX = p.sx;
    startY = lastY = p.sy;
    startWorld = w;
    moved = false;
    mode = tool === 'select' || boxKey ? 'box' : 'pan';
  };

  const onMove = (e: PointerEvent): void => {
    const p = local(e);
    cursor = p;
    if (!cursorPending) {
      cursorPending = true;
      requestAnimationFrame(publishCursor);
    }
    if (mode === 'idle') {
      if (session.ui.tool === 'ruler') {
        const w = engine.screenToWorld(p.sx, p.sy);
        session.rulerPreview(w.x, w.z);
      }
      const hit = engine.pickAtScreen(p.sx, p.sy);
      const id = hit?.id ?? null;
      const cur = session.ui.hover;
      if (id === null) {
        if (cur !== null) {
          session.store.setState({ hover: null });
          engine.setHover(null);
        }
      } else if (cur?.id !== id || Math.abs(cur.sx - p.sx) > 24 || Math.abs(cur.sy - p.sy) > 24) {
        session.store.setState({ hover: { id, sx: p.sx, sy: p.sy } });
        engine.setHover(id);
      }
      return;
    }
    if (e.pointerId !== pointerId) return;
    if (!moved && Math.hypot(p.sx - startX, p.sy - startY) < DRAG_PX) return;
    if (mode === 'pan') {
      if (!moved) session.dragged();
      moved = true;
      engine.panBy(p.sx - lastX, p.sy - lastY);
      lastX = p.sx;
      lastY = p.sy;
    } else {
      moved = true;
      const w = engine.screenToWorld(p.sx, p.sy);
      engine.setBox(dragRect(startWorld.x, startWorld.z, w.x, w.z));
    }
  };

  const onUp = (e: PointerEvent): void => {
    if (mode === 'idle' || e.pointerId !== pointerId) return;
    const p = local(e);
    if (host.hasPointerCapture(e.pointerId)) host.releasePointerCapture(e.pointerId);
    const was = mode;
    mode = 'idle';
    engine.setBox(null);
    if (was === 'box') {
      if (!moved) return;
      const w = engine.screenToWorld(p.sx, p.sy);
      const ids = engine.pickInBox(dragRect(startWorld.x, startWorld.z, w.x, w.z)).map((x) => x.id);
      session.select(ids, e.shiftKey ? 'add' : 'set');
      return;
    }
    if (moved) return;
    // a click
    const hit = engine.pickAtScreen(p.sx, p.sy);
    if (hit) session.select([hit.id], e.shiftKey ? 'toggle' : 'set');
    else if (!e.shiftKey) session.clearSelection();
  };

  const onLeave = (): void => {
    if (mode !== 'idle') return;
    if (session.ui.hover !== null) {
      session.store.setState({ hover: null });
      engine.setHover(null);
    }
    session.store.setState({ cursor: null });
  };

  const onWheel = (e: WheelEvent): void => {
    e.preventDefault();
    session.touch();
    if (e.deltaY === 0) return;
    const p = local(e);
    engine.zoomAt(e.deltaY < 0 ? WHEEL_STEP : 1 / WHEEL_STEP, p.sx, p.sy);
  };

  const onDouble = (e: MouseEvent): void => {
    session.touch();
    const p = local(e);
    engine.zoomAt(DOUBLE_CLICK_STEP, p.sx, p.sy);
  };

  const onContext = (e: MouseEvent): void => {
    session.touch();
    const p = local(e);
    const w = engine.screenToWorld(p.sx, p.sy);
    const hit = engine.pickAtScreen(p.sx, p.sy);
    const handled = session.contextMenu({
      clientX: e.clientX,
      clientY: e.clientY,
      world: session.ui.worldId,
      layer: session.ui.layer,
      x: w.x,
      z: w.z,
      entity: hit ? session.entity(hit.id) : null,
    });
    if (handled) e.preventDefault();
  };

  const onKeyUp = (e: KeyboardEvent): void => {
    if (e.key === 'b' || e.key === 'B') boxKey = false;
  };

  const onKey = (e: KeyboardEvent): void => {
    session.touch();
    if (typingInto(e.target) || e.ctrlKey || e.metaKey || e.altKey) return;
    const key = e.key;
    const step = e.shiftKey ? 240 : 80;
    const centre = (): { sx: number; sy: number } => ({
      sx: engine.view.w / 2,
      sy: engine.view.h / 2,
    });
    const handled = ((): boolean => {
      switch (key) {
        case 'ArrowLeft':
        case 'a':
        case 'A':
          engine.panBy(step, 0);
          return true;
        case 'ArrowRight':
        case 'd':
        case 'D':
          engine.panBy(-step, 0);
          return true;
        case 'ArrowUp':
        case 'w':
        case 'W':
          engine.panBy(0, step);
          return true;
        case 'ArrowDown':
        case 's':
        case 'S':
          engine.panBy(0, -step);
          return true;
        case '+':
        case '=':
          engine.zoomAt(WHEEL_STEP, centre().sx, centre().sy);
          return true;
        case '-':
        case '_':
          engine.zoomAt(1 / WHEEL_STEP, centre().sx, centre().sy);
          return true;
        case 'f':
        case 'F':
          session.toggleFollow();
          return true;
        case 'Escape':
          if (session.ui.picking !== null) session.cancelPick();
          else {
            session.clearSelection();
            if (session.ui.tool !== 'pan') session.setTool('pan');
          }
          return true;
        case '/':
          d.focusFind();
          return true;
        case 'l':
        case 'L':
          session.setTab('layers');
          return true;
        case 'n':
        case 'N':
          session.selectNext(e.shiftKey ? -1 : 1);
          return true;
        case 'm':
        case 'M':
          session.jumpToMe();
          return true;
        case 'b':
        case 'B':
          boxKey = true;
          return true;
        default:
          if (/^[1-9]$/.test(key)) {
            session.favouriteKey(Number(key) - 1);
            return true;
          }
          return false;
      }
    })();
    if (handled) {
      e.preventDefault();
      // the shell's own `/` (the command palette) must not also fire
      e.stopPropagation();
    }
  };

  const onAny = (): void => {
    session.touch();
  };

  host.addEventListener('pointerdown', onDown);
  host.addEventListener('pointermove', onMove);
  host.addEventListener('pointerup', onUp);
  host.addEventListener('pointercancel', onUp);
  host.addEventListener('pointerleave', onLeave);
  host.addEventListener('wheel', onWheel, { passive: false });
  host.addEventListener('dblclick', onDouble);
  host.addEventListener('contextmenu', onContext);
  window.addEventListener('keydown', onKey, true);
  window.addEventListener('keyup', onKeyUp, true);
  window.addEventListener('pointerdown', onAny, true);
  window.addEventListener('wheel', onAny, { capture: true, passive: true });

  return () => {
    host.removeEventListener('pointerdown', onDown);
    host.removeEventListener('pointermove', onMove);
    host.removeEventListener('pointerup', onUp);
    host.removeEventListener('pointercancel', onUp);
    host.removeEventListener('pointerleave', onLeave);
    host.removeEventListener('wheel', onWheel);
    host.removeEventListener('dblclick', onDouble);
    host.removeEventListener('contextmenu', onContext);
    window.removeEventListener('keydown', onKey, true);
    window.removeEventListener('keyup', onKeyUp, true);
    window.removeEventListener('pointerdown', onAny, true);
    window.removeEventListener('wheel', onAny, true);
  };
}
