// The `GCWAR_*` mode bits (WP-REND-26 / WP-INPUT-16; `Mover.cpp:3395-3446`, `WndWorld.cpp:4062-4088`): on the siege map a fighter sees only fighters, a
// waiting roster member sees the roster, everybody else sees everybody. The original recomputed two bits of `m_dwMode` of every PLAYER every frame:
//
//   GCWAR_NOT_CLICK_MODE    the pick returns nothing at all for the mover (`pickObject`, `PickSupport`)
//   GCWAR_RENDER_SKIP_MODE  the mover is not drawn, has no name and its chat is not shown (`decideDraw`, `chat/display.ts`)
//
// Pure rule here (`gcStatusOf`, `gcwarModeBits`); the module applies it over the entity table at the end of every drain.

import { EntityKind, type EntityTable } from '../../game/entities';
import { WI_WORLD_GUILDWAR } from '../../gen/defs/world';
import type { RosterGuild } from '../events/store';

/** `GCWAR_NOT_CLICK_MODE`. */
export const GCWAR_NOT_CLICK_MODE = 0x0008_0000;
/** `GCWAR_RENDER_SKIP_MODE`. */
export const GCWAR_RENDER_SKIP_MODE = 0x0010_0000;
/** Both bits: the only part of `visual.mode` this feature touches. */
export const GCWAR_MODE_BITS = GCWAR_NOT_CLICK_MODE | GCWAR_RENDER_SKIP_MODE;

/** `IsGCStatusPlayerWar`: -2 nobody is listed at all, -1 not on the roster, 0 waiting, 1 fighting. */
export type GcStatus = -2 | -1 | 0 | 1;

/**
 * `CWndWorld::IsGCStatusPlayerWar(uidPlayer)`. The C++ map has an entry for a defender only after its first `AddGCStatus`, so a roster guild with
 * no listed players counts as no entry; the first match wins, in roster order.
 */
export function gcStatusOf(roster: readonly RosterGuild[], charId: number): GcStatus {
  let listed = false;
  for (const g of roster) {
    for (const p of g.players) {
      listed = true;
      if (p.id === charId) return p.fighting ? 1 : 0;
    }
  }
  return listed ? -1 : -2;
}

/** The decision of the original: `bFlag`. */
export function hiddenFrom(viewer: GcStatus, subject: GcStatus): boolean {
  // a fighter sees only fighters; a waiting member sees the roster; everybody else (and a roster nobody is on) sees everybody
  if (viewer === 1) return subject !== 1;
  if (viewer === 0) return subject === -1;
  return false;
}

/** The two bits for a mover: both set or none. Only in world 202 and only for players. */
export function gcwarModeBits(
  worldId: number,
  viewer: GcStatus,
  subject: GcStatus,
  isPlayer = true,
): number {
  if (worldId !== WI_WORLD_GUILDWAR || !isPlayer) return 0;
  return hiddenFrom(viewer, subject) ? GCWAR_MODE_BITS : 0;
}

/** `mode` with exactly the two bits `bits` (0 or {@link GCWAR_MODE_BITS}); every other bit stays. */
export function withGcwarBits(mode: number, bits: number): number {
  return ((mode & ~GCWAR_MODE_BITS) | bits) >>> 0;
}

/**
 * Sets or clears the two bits of every player of the table for the viewer `viewerChar` (the own character id). Returns the number of movers whose word
 * changed. Characters are matched by `EntityRecord.idPlayer`; a player the table knows no character for is a roster outsider (-1).
 */
export function applyGcwarModes(
  entities: Pick<EntityTable, 'forEach'>,
  worldId: number,
  roster: readonly RosterGuild[],
  viewerChar: number,
): number {
  const viewer = gcStatusOf(roster, viewerChar);
  let changed = 0;
  entities.forEach(EntityKind.Player, (rec) => {
    const v = rec.visual;
    if (!v) return;
    const subject = gcStatusOf(roster, rec.idPlayer ?? 0);
    const next = withGcwarBits(v.mode, gcwarModeBits(worldId, viewer, subject));
    if (next !== v.mode >>> 0) {
      v.mode = next;
      changed += 1;
    }
  });
  return changed;
}
