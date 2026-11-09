// The M7 content visuals' triggers (Plan/41 section 19.1, Plan/22 section 17.2, Plan/45 section 28.10; WP-REND-27): the event balloon over a player and the wallpaper / carpet of the room the
// player stands in. Both are polled once per loop iteration by a tick system (cheap: a version compare per player, a memoised view), so the order of the module registrations does not matter
// and a renderer that appears later is told the current state.
//
//   eventBalloon   `fx.eventBalloon(playerId, itemId | null)`: the item of the FIRST `IK3_BALLOON` buff of a player (by buff key), `null` when it ended; players only
//   roomPapering   `fx.roomPapering({world, wall, file})` from `roomTexturesOf(ctx)` (the housing feature's view over the events store): every change, `world 0` = left the room
//
// The lord-skill effects (`LordSkillUseNtf`: `src_sfx` on the lord, `dst_sfx` on the target) are ui-lord's (`features/events/lord`); they go through `peekFxRuntime(ctx).fx().sfx` and nothing
// is added here.

import type { ClientContext } from '../../app/context';
import type { RenderFx } from '../../render/fx';
import { buffsOf } from '../../features/buffs/buffsModule';
import type { BuffSet } from '../../features/buffs/buffSet';
import { BUFF_ITEM, BUFF_ITEM2 } from '../../features/buffs/types';
import { NO_ROOM_TEXTURES, roomTexturesOf } from '../../features/events/housing/roomTextures';
import type { RoomTextures } from '../../features/events/housing/roomTextures';
import { eventOf } from '../../features/events/api';
import { IK3_BALLOON } from '../../gen/defs/itemkind';
import { EntityKind } from '../entities';
import type { FxRuntime } from './runtime';

/** The item of the first balloon buff of a set (by buff key), `null` without one. */
export function balloonItemOf(
  set: BuffSet | undefined,
  kind3Of: (itemId: number) => number | undefined,
): number | null {
  if (!set) return null;
  for (const b of set.list()) {
    if (b.type !== BUFF_ITEM && b.type !== BUFF_ITEM2) continue;
    if (kind3Of(b.id) === IK3_BALLOON) return b.id;
  }
  return null;
}

export function registerM7Handlers(
  ctx: ClientContext,
  rt: FxRuntime,
): () => Record<string, unknown> {
  // ---- the event balloon --------------------------------------------------------------------------------------------------------
  /** What the current renderer was told: player -> balloon item. */
  const told = new Map<number, number>();
  /** The buff set (and its version) a player was last evaluated with: an unchanged set is not evaluated again. */
  const seen = new Map<number, { set: BuffSet | undefined; version: number }>();
  let toldFx: RenderFx | undefined;
  const kind3Of = (id: number): number | undefined => ctx.data?.item(id)?.item_kind3;
  const balloons = (fx: RenderFx | undefined): void => {
    const buffs = buffsOf(ctx);
    if (!fx?.eventBalloon || !buffs) return;
    if (fx !== toldFx) {
      // a new renderer knows nothing yet
      toldFx = fx;
      told.clear();
      seen.clear();
    }
    const selfId = ctx.entities.self()?.objId ?? 0;
    const alive = new Set<number>();
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      alive.add(rec.objId);
      const set = rec.objId === selfId ? buffs.own() : buffs.of(rec.objId);
      const version = set?.version ?? -1;
      const prev = seen.get(rec.objId);
      if (prev && prev.set === set && prev.version === version) return;
      seen.set(rec.objId, { set, version });
      const item = balloonItemOf(set, kind3Of);
      const shown = told.get(rec.objId) ?? null;
      if (item === shown) return;
      rt.stats.balloons += 1;
      fx.eventBalloon?.(rec.objId, item);
      if (item === null) told.delete(rec.objId);
      else told.set(rec.objId, item);
    });
    for (const id of [...told.keys()]) {
      if (alive.has(id)) continue;
      fx.eventBalloon(id, null);
      told.delete(id);
    }
    for (const id of [...seen.keys()]) if (!alive.has(id)) seen.delete(id);
  };

  // ---- the room papering --------------------------------------------------------------------------------------------------------
  let view: ReturnType<typeof roomTexturesOf> | null = null;
  let sent: RoomTextures | null = null;
  let sentTo: RenderFx | undefined;
  const papering = (fx: RenderFx | undefined): void => {
    if (!fx?.roomPapering) return;
    // the view needs the events feature: built when it exists
    if (view === null && eventOf(ctx)) view = roomTexturesOf(ctx);
    const now = view?.get() ?? NO_ROOM_TEXTURES;
    if (
      sent !== null &&
      sentTo === fx &&
      sent.world === now.world &&
      sent.wall === now.wall &&
      sent.floor === now.floor
    ) {
      return;
    }
    // a fresh renderer with nothing to show needs no message
    if (sent === null && sentTo === undefined && now.world === 0) {
      sent = now;
      sentTo = fx;
      return;
    }
    sent = now;
    sentTo = fx;
    rt.stats.papering += 1;
    fx.roomPapering({ world: now.world, wall: now.wall, floor: now.floor });
  };

  ctx.tickSystems.register('fx.m7', () => {
    const fx = rt.fx();
    balloons(fx);
    papering(fx);
  });

  /** `__flyffDebug.fx.m7()`. */
  return () => ({
    balloons: [...told].map(([id, item]) => ({ id, item })),
    papering: sent === null ? null : { ...sent },
    stats: { balloons: rt.stats.balloons, papering: rt.stats.papering },
  });
}
