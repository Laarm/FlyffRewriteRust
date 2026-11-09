// Job priorities (Plan/40 section 8.3). The same order is used by the fetch scheduler (9.2), the worker
// dispatcher and the upload queue (9.6), so a Critical item is never stuck behind Statics at any stage.

export const Prio = {
  /** Boot bundles before login; the own player's skeleton/parts/clips; the loading-gate lands. */
  Critical: 0,
  /** Remaining boot bundles; window lands outside the gate set. */
  Near: 1,
  /** Movers in view: parts, skeletons, clips, textures (ordered by distance). */
  Visible: 2,
  /** A sound or music file a play request is waiting for. */
  AudioDemand: 3,
  /** Static objects of loaded lands, nearest land first. */
  Statics: 4,
  /** SFX files and textures. */
  Fx: 5,
  /** Prefetch ring lands, lazy bundles, previews, /dev content. */
  Prefetch: 6,
  /** Sound prefetch. */
  AudioPrefetch: 7,
} as const;
export type Prio = (typeof Prio)[keyof typeof Prio];

/** What orders two queued items: class first, then lower `distance`, then FIFO (`seq`). */
export interface Rank {
  readonly prio: Prio;
  readonly distance: number;
  readonly seq: number;
}

/** Negative when `a` must run before `b`. */
export function compareRank(a: Rank, b: Rank): number {
  return a.prio - b.prio || a.distance - b.distance || a.seq - b.seq;
}
