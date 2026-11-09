// The DOM face of the window manager (Plan/42 §4.8, §5.3): renders the frames in z-order, the modal blocker, and
// translates browser pointer events into the virtual-pixel calls of `WindowManager`. The root element is
// transparent to the pointer; the listeners sit on the document so that a click on the 3-D canvas still reaches the
// manager (it blurs the focused frame and closes popups) while the canvas handles the event itself. The press, move and wheel
// that the manager consumes are marked (`wm/pointerClaims.ts`) and the world input skips them (BFIX1 #004).

import { type ReactNode, useEffect, useRef, useSyncExternalStore } from 'react';
import type { CursorManager } from './cursor/CursorManager';
import { DragLayer } from './dnd/DragLayer';
import { installIconAnimations } from './icons/anim';
import type { UiOptionsStore } from './options';
import type { UiImageCache } from './skin/images';
import type { UiRamp } from './skin/ramp';
import { TitleBackdrop } from './skin/TitleBackdrop';
import { TooltipLayer } from './tooltip/TooltipLayer';
import type { TooltipService } from './tooltip/TooltipService';
import { FrameWindowView } from './wnd/FrameWindowView';
import type { PointerInfo } from './wnd/WndBase';
import { isPixelatedScale, type UiScaler } from './wm/scale';
import { markUiClaimed } from './wm/pointerClaims';
import type { WindowManager } from './wm/WindowManager';
import './styles/ui.css';

export interface UiRootProps {
  wm: WindowManager;
  /** `WindowAlpha` (0-255) of the skin canvases; wins over `options`. Title screens always use 255. */
  alpha?: number;
  /** CSS scale of the whole UI; wins over `scaler`. */
  scale?: number;
  /** The scale and the virtual viewport of the browser window (`UiScaler`, §16): the root follows its scale. */
  scaler?: UiScaler;
  /** `windowAlpha` comes from here when `alpha` is not given. */
  options?: UiOptionsStore;
  /** The screen ramp (§7.8): its filter is put on the root element. */
  ramp?: UiRamp;
  /** A title state (login, character select/create): paints the wallpaper (§7.7) and forces `WindowAlpha` 255. */
  title?: boolean;
  /** Adds the dev outline of every control. */
  outline?: boolean;
  /** The event target of the pointer listeners (default `document`). */
  target?: Document | HTMLElement;
  /** The tooltip host and the image cache of the tooltip frame: the layer is rendered above every window. */
  tooltip?: { service: TooltipService; images: UiImageCache };
  /** The cursor registry: its UI target is this root (windows inherit the cursor). */
  cursor?: CursorManager;
  /**
   * Feeds `keydown`/`keyup` of the document to `wm.keyDown/keyUp` (Enter, Space, Tab of `WBS_KEY` frames). The game
   * mounts 43's key router instead; the gallery and the tests use this.
   */
  keys?: boolean;
  /** Drawn UNDER the windows, over the 3-D view (the world-map overlay of 45 §7.7): it must not take the pointer. */
  underlay?: ReactNode;
}

function rootClass(outline: boolean, pixelated: boolean): string {
  let c = 'ui-root';
  if (pixelated) c += ' ui-pixelated';
  if (outline) c += ' ui-dev-outline';
  return c;
}

function info(e: PointerEvent | MouseEvent): PointerInfo {
  return { button: e.button, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey };
}

const NO_SUBSCRIBE = (): (() => void) => () => undefined;

export function UiRoot({
  wm,
  alpha: alphaProp,
  scale: scaleProp,
  scaler,
  options,
  ramp,
  title = false,
  outline = false,
  target,
  tooltip,
  cursor,
  keys = false,
  underlay,
}: UiRootProps) {
  const snap = useSyncExternalStore(wm.subscribe, wm.getSnapshot);
  const scalerScale = useSyncExternalStore<number | null>(
    scaler?.subscribe ?? NO_SUBSCRIBE,
    scaler ? () => scaler.get().scale : () => null,
  );
  // only the field this view renders: a change of `gamma` etc. must not re-render the root (the ramp writes the DOM)
  const optAlpha = useSyncExternalStore<number>(
    options?.subscribe ?? NO_SUBSCRIBE,
    options ? () => options.get().windowAlpha : () => 255,
  );
  const scale = scaleProp ?? scalerScale ?? 1;
  const alpha = title ? 255 : (alphaProp ?? optAlpha);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    return ramp && el ? ramp.attachRoot(el) : undefined;
  }, [ramp]);

  useEffect(() => {
    installIconAnimations(); // the cooldown / enchant / drag-pulse CSS (idempotent)
  }, []);

  useEffect(() => {
    const el = target ?? document;
    const toVirtual = (e: MouseEvent): { x: number; y: number } => {
      const root = ref.current;
      const box = root ? root.getBoundingClientRect() : { left: 0, top: 0 };
      return {
        x: Math.round((e.clientX - box.left) / scale),
        y: Math.round((e.clientY - box.top) / scale),
      };
    };
    // what the manager consumes (a window or an icon under the pointer, a modal, a running drag) is marked on the DOM event, so the
    // listener of the 3-D canvas behind this one (the document capture phase comes first) can leave it alone (`pointerClaims`, #004).
    // Releases and cancels are never marked, nothing is stopped: other listeners and React keep working.
    const down = (ev: Event): void => {
      const e = ev as PointerEvent;
      if (wm.pointerDown(toVirtual(e), info(e)).consumed) markUiClaimed(e);
    };
    const move = (ev: Event): void => {
      const e = ev as PointerEvent;
      if (wm.pointerMove(toVirtual(e), info(e)).consumed) markUiClaimed(e);
    };
    const up = (ev: Event): void => {
      const e = ev as PointerEvent;
      wm.pointerUp(toVirtual(e), info(e));
    };
    const cancel = (): void => {
      wm.pointerCancel();
    };
    const wheel = (ev: Event): void => {
      const e = ev as WheelEvent;
      const r = wm.wheel(toVirtual(e), e.deltaY < 0 ? 1 : e.deltaY > 0 ? -1 : 0);
      if (r.consumed) markUiClaimed(e);
      if (r.consumed && r.frame) e.preventDefault();
    };
    el.addEventListener('pointerdown', down, true);
    el.addEventListener('pointermove', move, true);
    el.addEventListener('pointerup', up, true);
    el.addEventListener('pointercancel', cancel, true);
    el.addEventListener('wheel', wheel, { capture: true, passive: false });
    return () => {
      el.removeEventListener('pointerdown', down, true);
      el.removeEventListener('pointermove', move, true);
      el.removeEventListener('pointerup', up, true);
      el.removeEventListener('pointercancel', cancel, true);
      el.removeEventListener('wheel', wheel, true);
    };
  }, [wm, scale, target]);

  useEffect(() => {
    if (!cursor) return;
    cursor.setUiTarget(ref.current);
    return () => {
      cursor.setUiTarget(null);
    };
  }, [cursor]);

  useEffect(() => {
    if (!keys) return;
    const kinfo = (e: KeyboardEvent): { shift: boolean; ctrl: boolean; alt: boolean } => ({
      shift: e.shiftKey,
      ctrl: e.ctrlKey,
      alt: e.altKey,
    });
    const down = (e: KeyboardEvent): void => {
      if (wm.keyDown(e.code, kinfo(e))) e.preventDefault();
    };
    const up = (e: KeyboardEvent): void => {
      if (wm.keyUp(e.code, kinfo(e))) e.preventDefault();
    };
    document.addEventListener('keydown', down, true);
    document.addEventListener('keyup', up, true);
    return () => {
      document.removeEventListener('keydown', down, true);
      document.removeEventListener('keyup', up, true);
    };
  }, [wm, keys]);

  const vp = snap.viewport;
  const modalIndex = snap.modal ? snap.order.indexOf(snap.modal) : -1;
  const zOf = new Map(snap.order.map((f, i) => [f, i] as const));
  const stableOrder = [...snap.order].sort((a, b) => a.uid - b.uid);
  return (
    <div
      ref={ref}
      className={rootClass(outline, isPixelatedScale(scale))}
      data-testid="ui-root"
      style={{
        width: vp.r - vp.l,
        height: vp.b - vp.t,
        transform: scale === 1 ? undefined : `scale(${String(scale)})`,
      }}
    >
      {title && <TitleBackdrop env={wm.env} width={vp.r - vp.l} height={vp.b - vp.t} />}
      {underlay}
      {modalIndex >= 0 && (
        <div
          className="wnd-modal-blocker"
          data-modal-blocker="true"
          style={{ zIndex: modalIndex * 10 - 1 }}
        />
      )}
      {/*
        The frames are mounted in a STABLE DOM order (by creation); the z-order is the `z-index`. A DOM order that follows the z-order
        makes React move the node of a window that is raised by a click, and a moved node loses the native focus of its edit.
      */}
      {stableOrder.map((f) => (
        <FrameWindowView
          key={f.uid}
          frame={f}
          z={(zOf.get(f) ?? 0) * 10}
          alpha={alpha}
          focused={snap.focusFrame === f}
        />
      ))}
      <DragLayer controller={wm.drag} env={wm.env} />
      {tooltip && <TooltipLayer service={tooltip.service} images={tooltip.images} />}
    </div>
  );
}
