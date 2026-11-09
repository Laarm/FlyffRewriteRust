// The render view as the effects read it (`FrameInput.view` is typed only as `{ frame }`): narrow, null-safe adapters. A dev page or a
// test passes a view without the sim's readers - or none at all - and the effects that need them then do nothing.

import type { RenderViewReaderLike } from '../types';
import type { ProjectileView } from './Projectiles';
import type { WaterMovers } from './waterFx';

/** The projectile area of a render view, `null` when the view has none. */
export function asProjectileView(view: RenderViewReaderLike | null): ProjectileView | null {
  const v = view as Partial<ProjectileView> | null;
  return v && v.projectiles instanceof Float32Array && typeof v.projCount === 'number'
    ? { projectiles: v.projectiles, projCount: v.projCount }
    : null;
}

/** The mover readers the water effects need, `null` for a view that has none (or that is not valid yet). */
export function asWaterMovers(view: RenderViewReaderLike | null): WaterMovers | null {
  if (!view) return null;
  const v = view as Partial<WaterMovers> & { valid?: boolean };
  return typeof v.forEachMover === 'function' &&
    typeof v.posOf === 'function' &&
    typeof v.stateOf === 'function' &&
    v.valid !== false
    ? (view as unknown as WaterMovers)
    : null;
}
