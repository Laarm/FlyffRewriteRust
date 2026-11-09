// The region under the own player (`worldStore.region`, Plan/40 section 6.2.3, 6.4: 4 Hz from the self position; Plan/15 section
// 11.2 is the server's twin of this rule): the title of the place, the attribute flags (`RA_*`), the safe-zone flag and the
// music of the regions of the `worlds` bundle that contain the position. Containment is `left <= x < right && top <= z <
// bottom` on the TRUNCATED position (`CRect::PtInRect` of `(LONG)` values); the attribute is the OR of every containing
// region, and the world default (`pk_mode` of the `.wld`) when none of SAFETY | PENALTY_PK | PK | FIGHT is set.

import type { ClientRegion } from '../../gen/bundles';
import { RA_FIGHT, RA_PENALTY_PK, RA_PK, RA_SAFETY } from '../../gen/defs/define';
import { RI_ATTRIBUTE, RI_PLACE, RI_TRIGGER } from '../../gen/defs/obj';

/** `worldStore.region`. */
export interface RegionState {
  readonly title: string | null;
  readonly attribute: number;
  readonly safe: boolean;
  readonly music: number;
}

/** The regions the client evaluates (`RI_TRIGGER`, `RI_ATTRIBUTE`, `RI_PLACE`; the others are not stored in the world). */
const KINDS: ReadonlySet<number> = new Set([RI_TRIGGER, RI_ATTRIBUTE, RI_PLACE]);
const CLASS_BITS = RA_SAFETY | RA_PENALTY_PK | RA_PK | RA_FIGHT;

export const NO_REGION: RegionState = { title: null, attribute: 0, safe: false, music: 0 };

/** The region state at `(x, z)`. */
export function regionAt(
  regions: readonly ClientRegion[],
  worldPkMode: number,
  x: number,
  z: number,
): RegionState {
  const px = Math.trunc(x);
  const pz = Math.trunc(z);
  let attr = 0;
  let title: string | null = null;
  let music = 0;
  for (const r of regions) {
    if (!KINDS.has(r.index)) continue;
    const [left, top, right, bottom] = r.rect;
    if (px < left || px >= right || pz < top || pz >= bottom) continue;
    attr |= r.attribute;
    if (r.title !== null && r.title !== '') title = r.title;
    if (r.music !== 0) music = r.music;
  }
  const attribute = (attr & CLASS_BITS) !== 0 ? attr : worldPkMode;
  return { title, attribute, safe: (attribute & RA_SAFETY) !== 0, music };
}

export function sameRegion(a: RegionState, b: RegionState): boolean {
  return (
    a.title === b.title && a.attribute === b.attribute && a.safe === b.safe && a.music === b.music
  );
}
