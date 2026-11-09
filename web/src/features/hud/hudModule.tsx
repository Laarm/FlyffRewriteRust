// The `hud` feature module (Plan/45 §7.11; WP-CFEAT-07): owns the HUD message state of a context, steps it with the 60 Hz tick
// systems and mounts its DOM overlay between the world canvas and the UI windows. Other features reach it through
// `hudOf(ctx)` (the chat router's CLIENT target, `DefinedText` captions, the region captions of the world).

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ClientContext } from '../../app/context';
import {
  APP_CHAT_FILTER,
  APP_CHAT_LOG,
  APP_COMMAND,
  APP_COMMUNICATION_CHAT,
  APP_MESSAGE_TEXT,
} from '../../gen/defs/resdata';
import type { FeatureModule } from '../common/module';
import { fullUi } from '../common/module';
import { HudOverlay } from './HudOverlay';
import { HudState } from './hud';

const states = new WeakMap<ClientContext, HudState>();

/** The HUD message state of a context (created on first use, also before the module registered). */
export function hudOf(ctx: ClientContext): HudState {
  let s = states.get(ctx);
  if (!s) {
    s = new HudState();
    states.set(ctx, s);
  }
  return s;
}

/** The DOM measurer of caption extents (one canvas 2-D context); `null` without a canvas. */
export function domMeasure(): HudState['measure'] | null {
  if (typeof document === 'undefined') return null;
  const ctx = document.createElement('canvas').getContext('2d');
  if (!ctx) return null;
  return (text, px) => {
    ctx.font = `${String(px)}px "Flyff DejaVu Sans", Verdana, sans-serif`;
    return { w: Math.round(ctx.measureText(text).width), h: Math.round(px * 1.25) };
  };
}

/** Mounts the overlay container in the document; returns the unmount. */
export function mountHudOverlay(ctx: ClientContext, hud: HudState): () => void {
  if (typeof document === 'undefined') return () => undefined;
  const el = document.createElement('div');
  el.dataset['testid'] = 'hud-root';
  el.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;pointer-events:none;z-index:0';
  const world = document.getElementById('world');
  if (world?.parentNode) world.parentNode.insertBefore(el, world.nextSibling);
  else document.body.append(el);
  const root: Root = createRoot(el);
  const render = (): void => {
    const ui = fullUi(ctx);
    const st = ui?.scaler.get();
    // the caption fonts derive from the virtual width (`rectClient.Width() / nDivCaption`)
    hud.viewportWidth = st?.virtual.width ?? 1024;
    root.render(
      createElement(HudOverlay, {
        hud,
        viewport: st?.virtual ?? { width: 1024, height: 768 },
        scale: st?.scale ?? 1,
      }),
    );
  };
  render();
  const off = fullUi(ctx)?.scaler.subscribe(render);
  return () => {
    off?.();
    root.unmount();
    el.remove();
  };
}

/** The windows `openField` opens that this task owns: the taskbar, the chat windows and the logout countdown. */
export function closeFieldWindows(ctx: ClientContext): void {
  const ui = fullUi(ctx);
  if (!ui) return;
  ui.taskbar.destroy();
  for (const app of [
    APP_COMMAND,
    APP_CHAT_FILTER,
    APP_CHAT_LOG,
    APP_COMMUNICATION_CHAT,
    APP_MESSAGE_TEXT,
  ]) {
    ui.wm.close(app);
  }
}

export const hudModule: FeatureModule = {
  id: 'hud',
  milestone: 2,
  register(fc) {
    const ctx = fc.app;
    const hud = hudOf(ctx);
    const measure = domMeasure();
    if (measure) hud.measure = measure;
    ctx.tickSystems.register('hud', (due) => {
      hud.frame(due, ctx.now());
    });
    ctx.machine.subscribe((screen, previous) => {
      const reattach = previous.s === 'Reconnecting' || screen.s === 'Reconnecting';
      if (screen.s === 'InGame' && previous.s !== 'InGame' && !reattach) {
        // the world is entered: `CWndMgr::OpenField` (the taskbar, an empty chat log, the chat window, the navigator and the
        // status window of those that are built)
        fullUi(ctx)?.wm.openField();
      } else if (previous.s === 'InGame' && screen.s !== 'InGame' && !reattach) {
        // the world is left: nothing floats over the next one and the HUD windows of this task go
        hud.clear();
        closeFieldWindows(ctx);
      }
    });
    if (typeof document !== 'undefined' && ctx.caps.webAssembly) {
      // the overlay is mounted lazily with the first message so that title screens and tests without a UI pay nothing
      let off: (() => void) | null = null;
      hud.subscribe(() => {
        off ??= mountHudOverlay(ctx, hud);
      });
    }
  },
};
