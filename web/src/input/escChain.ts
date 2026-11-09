// The Esc priority chain (Plan/43 §3.6; `WndManager.cpp:3379-3515`, `WndWorld.cpp:7900-7906`; WP-INPUT-08). Runs in the
// `worldTick` when `Escape` is pending (and on the key-up for the world map). The first applicable step acts and ends the
// chain; doc 42 owns the window operations of steps 8-9, this module owns the order and the game steps.
//
//   1  the world-map overlay is shown                      -> hide it
//   2  the player is dead                                  -> open the revival window (unless it is open)
//   3  the start menu is visible                           -> nothing else happens
//   4  a next target is queued                             -> drop it (a hostile focus becomes the render target)
//   5  a focus exists                                      -> clear focus, render target, destination, auto-attack, tracking
//   6  the quest-NPC destination arrow is on               -> clear it
//   7  create-monster placement mode                       -> leave it
//   8  a shortcut icon is being dragged                    -> nothing
//   9  the top closable window                             -> close it (mover menu / window 10: focus the world instead);
//      no window                                           -> open the start menu above the taskbar

import type { LocalCommands } from './commands';
import type { InputHooks, WorldWindows } from './ports';
import type { Targeting } from './targeting';

/** Which step of the chain acted (the tests and the probe read it). */
export type EscStep =
  | 'worldMap'
  | 'revival'
  | 'startMenu'
  | 'nextTarget'
  | 'focus'
  | 'questDestination'
  | 'createMonster'
  | 'drag'
  | 'focusWorld'
  | 'kept'
  | 'closeWindow'
  | 'openStartMenu';

export interface EscChainDeps {
  readonly windows: Pick<
    WorldWindows,
    'isDraggingShortcut' | 'closeTopWindow' | 'topClosableWindow' | 'openStartMenu'
  > & { topIsMenuOrWorldFocus?(): boolean; focusWorld?(): void };
  readonly hooks: InputHooks;
  readonly targeting: Targeting;
  readonly commands: Pick<LocalCommands, 'clearDest' | 'stopTracking'>;
  /** The own player is dead. */
  dead(): boolean;
  /** `MVRF_TRACKING` is on. */
  tracking(): boolean;
  /** `true` when `id` is a mover that can be attacked (the render-target rule of step 4). */
  hostileMover(id: number): boolean;
}

/** Runs the chain once; returns the step that acted. */
export function escChain(d: EscChainDeps): EscStep {
  const h = d.hooks;
  const t = d.targeting;
  if (h.worldMap?.shown() === true) {
    h.worldMap.hide();
    return 'worldMap';
  }
  if (d.dead()) {
    if (h.revival && !h.revival.isOpen()) h.revival.open();
    return 'revival';
  }
  if (h.startMenuVisible?.() === true) return 'startMenu';
  if (t.nextTarget !== null) {
    t.nextTarget = null;
    if (t.focus !== null && d.hostileMover(t.focus)) t.renderTarget = t.focus;
    return 'nextTarget';
  }
  if (t.focus !== null) {
    h.cancelHeroCast?.();
    t.setFocus(null);
    t.renderTarget = null;
    t.focusOld = null;
    d.commands.clearDest();
    t.autoAttack = false;
    if (d.tracking()) d.commands.stopTracking();
    return 'focus';
  }
  if (h.questDestination?.active() === true) {
    h.questDestination.clear();
    return 'questDestination';
  }
  if (h.createMonster?.active() === true) {
    h.createMonster.leave();
    return 'createMonster';
  }
  if (d.windows.isDraggingShortcut()) return 'drag';
  const top = d.windows.topClosableWindow();
  if (top !== null) {
    if (d.windows.topIsMenuOrWorldFocus?.() === true) {
      d.windows.focusWorld?.();
      return 'focusWorld';
    }
    const r = d.windows.closeTopWindow();
    return r === 'kept' ? 'kept' : 'closeWindow';
  }
  d.windows.openStartMenu(true);
  return 'openStartMenu';
}
