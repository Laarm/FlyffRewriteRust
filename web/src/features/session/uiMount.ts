// Mounting the UI (`ui/NOTES.md` "Mounting", Plan/40 §3.1 B5, 42 §2.3): `createGameUi(ctx)` is the B5 step that builds the UI
// framework over `uiHost(ctx)` with the options port of 45's store and the session hooks, and wires what `createUi` leaves
// to the host: the debug probe section, the scaler's browser observation, the title / game mode, the saving of the window
// rects when the world is left and the closing of every in-game window then (`DestroyApplet` + `Free` of the original's
// `OpenTitle(FALSE)`; BFIX1 #021). `UiLayer` (React) renders `UiRoot`.

import type { ClientContext } from '../../app/context';
import { uiHost } from '../../app/hosts';
import { ddsToPixelImage } from '../../ui/skin/ddsImage';
import { createBrowserDecoder } from '../../ui/skin/images';
import { Prio } from '../../workers/priority';
import { quitSession } from '../../game/session/actions';
import { createUi, type UiApi } from '../../ui';
import { setLazyWindowLog } from '../../ui/wnd/lazy';
import { defineOptionsStore, setOptionByPath, uiOptionsPort } from '../options';
import type { ScreenName, ScreenState } from '../../stores/screen';
import { TITLE_SCREENS } from './UiLayer';

/** The states in which the title wallpaper shows and the taskbar inset is 0 (`UiScaler.setMode('title')`). */
const isTitle = (screen: ScreenState): boolean => TITLE_SCREENS.has(screen.s);

/**
 * The states whose windows belong to the field (`WindowManager.setFieldScope`): a frame inserted in one of them is closed
 * when the world is left. `Entering` stays out on purpose, so the lobby's Connecting box is not swept.
 */
const FIELD_SCREENS: ReadonlySet<ScreenName> = new Set<ScreenName>([
  'Loading',
  'InGame',
  'Reconnecting',
  'Leaving',
]);
/** The states in which the session is in the world (the windows stay on a world change, a reconnect and a reattach). */
const WORLD_SCREENS: ReadonlySet<ScreenName> = new Set<ScreenName>([
  'InGame',
  'Loading',
  'Reconnecting',
]);

/**
 * The id of the last character that was in the world, per context: `toLogin` / `charList` reset the world scope (and with it
 * `selfStore.idPlayer`) BEFORE the screen listeners run, so the rect save of a quit, a kick or a logout needs this
 * (`uiHost`'s `fallbackPlayerId`). Forgotten when the next `Entering` starts.
 */
const lastWorldPlayer = new WeakMap<ClientContext, number>();

/** Builds the UI framework of the context (`ctx.ui` is assigned by the caller). */
export function createGameUi(ctx: ClientContext): UiApi {
  const options = defineOptionsStore(ctx.stores.registry);
  const host = uiHost(ctx, {
    options: uiOptionsPort(options),
    fallbackPlayerId: () => lastWorldPlayer.get(ctx) ?? null,
    // the `sfx/` atlases (`sfx/countfight.inc`, the PvP countdown sprites) have no PNG form: their DDS goes through the decode pool as RGBA8
    decode: createBrowserDecoder({
      dds: async (bytes) => {
        const pool = ctx.workers;
        if (!pool) return null;
        const r = await pool.submit({ kind: 'dds_bc', bytes, prio: Prio.Visible }).promise;
        return r.blob ? ddsToPixelImage({ meta: r.meta, blob: new Uint8Array(r.blob) }) : null;
      },
    }),
    dialogs: {
      // `TID_DIAG_0035` ("Old version of client."): the original exited the client; the browser goes to the login state
      returnToLogin: () => {
        void quitSession(ctx);
      },
      // `WM_LOGOUT` of an upper message box (the legacy `OpenTitle(FALSE)`)
      logout: () => {
        void quitSession(ctx);
      },
    },
  });
  const ui = createUi(host);
  wireUi(ctx, ui);
  return ui;
}

/** The one-time wiring after `createUi` (probe section, scaler, mode follow-up, rect persistence). */
export function wireUi(ctx: ClientContext, ui: UiApi): void {
  if (!ctx.probe.sections().includes('ui')) ctx.probe.extend('ui', ui.probe);
  // the dialog windows load in the background (the title screens do not need them); the loading gate of the world waits for them
  setLazyWindowLog((message) => {
    ctx.log.warn('ui', message);
  });
  void ui.preloadWindows().then((failed) => {
    if (failed.length > 0) ctx.log.warn('ui', `windows that did not load: ${failed.join(', ')}`);
  });
  ui.scaler.observeBrowser();
  const applyMode = (screen: ScreenState): void => {
    ui.scaler.setMode(isTitle(screen) ? 'title' : 'game');
  };
  applyMode(ctx.machine.screen);
  // the field scope: what is inserted while the session is in the world belongs to the field. The machine sets `screen` before
  // effects and listeners run, so a title window that the screen controller opens in the same transition is never tagged.
  ui.wm.setFieldScope(() => FIELD_SCREENS.has(ctx.machine.screen.s));
  ctx.machine.subscribe((screen, previous) => {
    applyMode(screen);
    // the character of the window-rect key (the self store is already reset when a quit, a kick or a logout is heard)
    if (screen.s === 'Entering') {
      lastWorldPlayer.delete(ctx);
    } else if (screen.s === 'InGame' || screen.s === 'Loading') {
      const id = ctx.stores.selfStore.getState().idPlayer;
      if (id) lastWorldPlayer.set(ctx, id);
    }
    // the world is left (logout, kick, login, a failed reconnect): the window positions are stored for the next time
    const saveRects =
      previous.s === 'InGame' && screen.s !== 'InGame' && screen.s !== 'Reconnecting';
    const leftWorld = WORLD_SCREENS.has(previous.s) && !WORLD_SCREENS.has(screen.s);
    if (saveRects || leftWorld) ui.regInfo.saveAll();
    if (leftWorld) {
      // `OpenTitle(FALSE)` = `DestroyApplet` + `Free`: no in-game window survives onto the title screens (no world change,
      // reconnect or reattach: those keep their windows). The taskbar object also goes (the hud skips a failed reconnect).
      ui.wm.closeField();
      ui.tooltip.cancel();
      ui.taskbar.destroy();
    } else if (previous.s === 'Leaving' && !FIELD_SCREENS.has(screen.s)) {
      // windows that late world messages opened while the character was leaving
      ui.wm.closeField();
      ui.tooltip.cancel();
    }
  });
  const options = defineOptionsStore(ctx.stores.registry);
  ctx.probe.setTestHook('setOption', (path, value) => {
    setOptionByPath(options, path, value);
  });
}
