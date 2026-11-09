// The ground position of an item dropped on the 3-D view (`CWndWorld::OnDropIcon`, `WndWorld.cpp:8025-8040`): one metre from the
// player towards the point under the pointer, a little above the ground; the player's own position when the pointer points at
// no ground. The server clamps the position to 3 m from the player and uses its own height (PL-078), so the exact numbers only
// decide where the dropped item appears.

import type { ClientContext } from '../../../app/context';
import { groundPick, pickRayLH, type PickCamera } from '../../../input/picking';
import type { Point } from '../../../ui/layout/geometry';
import { fullUi } from '../../common/module';
import type { Position } from '../drop/drop';

/** The part of the input instance the pick reads (the displayed camera and the canvas size). */
interface PickSource {
  core?: {
    displayed: PickCamera | null;
    deps: { viewport(): { w: number; h: number } };
  };
}

/** The player's logical position (the 4 Hz store value; the sim view is not needed for a 3 m clamp). */
function selfPosition(ctx: ClientContext): Position {
  const p = ctx.stores.selfStore.getState().pos;
  return [p[0], p[1], p[2]];
}

/** `pt` is in UI virtual pixels; the pick ray works in canvas CSS pixels. */
export function groundPosition(ctx: ClientContext, pt: Point): Position {
  const me = selfPosition(ctx);
  const src = ctx.input as unknown as PickSource;
  const cam = src.core?.displayed ?? null;
  if (!cam || !src.core) return me;
  const vp = src.core.deps.viewport();
  if (vp.w <= 0 || vp.h <= 0) return me;
  const scale = fullUi(ctx)?.scaler.get().scale ?? 1;
  const ray = pickRayLH(cam, pt.x * scale, pt.y * scale, vp.w, vp.h);
  const hit = groundPick((o, d, t, m) => ctx.sim.rayCast(o, d, t, m), ray);
  if (!hit) return me;
  const dx = hit.pos[0] - me[0];
  const dy = hit.pos[1] - me[1];
  const dz = hit.pos[2] - me[2];
  const len = Math.hypot(dx, dy, dz);
  if (!(len > 0)) return me;
  // the unit vector towards the point, from the player; the height is the player's plus 0.2 (`fHeight + 0.2` when the ground
  // is within a metre of the player, else the player's position)
  return [me[0] + dx / len, me[1] + 0.2, me[2] + dz / len];
}
