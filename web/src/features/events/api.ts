// The public face of the events feature (`eventOf( ctx )`), in a module of its own so that the features that only READ the flags (the NPC menu) import it without the module.

import type { ClientContext } from '../../app/context';
import type { EventStore, HudTimerSource } from './store';

export interface EventFeature {
  readonly store: EventStore;
  /** `g_eLocal.GetState( id ) != 0` (22 §3). */
  isEventOn(id: number): boolean;
  /**
   * Writes the ONE HUD timer line: `label` with the time left, counted from now (`endMs = now + ms`); the last writer wins. `waitSec` shows the seconds with
   * `TID_GAME_GUILDCOMBAT_1TO1_WAITSEC` (the 1:1 variant).
   */
  setHudTimer(label: string, ms: number, src: HudTimerSource, waitSec?: boolean): void;
  /** Clears the timer; with `src` only when that writer wrote it. */
  clearHudTimer(src?: HudTimerSource): void;
}

const features = new WeakMap<ClientContext, EventFeature>();

/** The events feature of a context; `undefined` before the module registered. */
export function eventOf(ctx: ClientContext): EventFeature | undefined {
  return features.get(ctx);
}

/** Called by the module once it exists. */
export function setEventOf(ctx: ClientContext, feature: EventFeature): void {
  features.set(ctx, feature);
}
