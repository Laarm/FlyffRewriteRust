// The continent test of the client (`CContinent::GetContinent( pos )` and `GetMapNo`, `src:_Common/Continent.cpp:219-254, 288-300, 414-430`;
// Plan/14 §10.5, 45 §7.7): which `.wld.cnt` polygon holds a position, and from it the map number of the world-map overlay. The polygons come
// from the `worlds` bundle (`folders.wdmadrigal.continents`), the continent -> map rule from `worlds.world_maps` (`continent` of each row), so
// there is no table of continents in this file: only the integer crossing test of `Point_In_Poly`, which has to match the server's.

import type { ContinentDef, WorldMapRow } from '../../gen/bundles';
import { WI_WORLD_MADRIGAL } from '../../gen/defs/world';

/** `CONT_NODATA`. */
export const CONT_NODATA = 0;
/** "No map available" (`GetMapNo` default). */
export const MAP_NONE = 100;

export interface ContinentPoly {
  readonly id: number;
  /** The closed polygon as integer points `((int)x, (int)z)`. */
  readonly points: readonly (readonly [number, number])[];
}

/** The server continents of the `.wld.cnt` blocks in ascending id order (only `C_useRealData` blocks, towns excluded). */
export function continentPolys(defs: readonly ContinentDef[]): ContinentPoly[] {
  const byId = new Map<number, ContinentPoly>();
  for (const d of defs) {
    if (!d.use_real_data || d.town || d.vertices.length === 0) continue;
    if (d.id < 0 || d.id > 255 || byId.has(d.id)) continue; // `map::insert` keeps the first block of an id
    const points: [number, number][] = d.vertices.map((v) => [Math.trunc(v[0]), Math.trunc(v[2])]);
    const first = points[0];
    if (first) points.push([first[0], first[1]]); // both original readers closed the polygon
    byId.set(d.id, { id: d.id, points });
  }
  return [...byId.values()].sort((a, b) => a.id - b.id);
}

/**
 * `CContinent::Point_In_Poly`: the crossing number with INTEGER arithmetic (`xinters = (z - p1.z) * (p2.x - p1.x) / (p2.z - p1.z) + p1.x`
 * with truncating division). An empty polygon contains nothing.
 */
export function pointInPoly(
  poly: readonly (readonly [number, number])[],
  x: number,
  z: number,
): boolean {
  const n = poly.length;
  if (n === 0) return false;
  let counter = 0;
  let p1 = poly[0];
  if (!p1) return false;
  for (let i = 1; i <= n; i += 1) {
    const p2 = poly[i % n];
    if (!p2) return false;
    if (
      z > Math.min(p1[1], p2[1]) &&
      z <= Math.max(p1[1], p2[1]) &&
      x <= Math.max(p1[0], p2[0]) &&
      p1[1] !== p2[1]
    ) {
      const xinters = Math.trunc(((z - p1[1]) * (p2[0] - p1[0])) / (p2[1] - p1[1])) + p1[0];
      if (p1[0] === p2[0] || x <= xinters) counter += 1;
    }
    p1 = p2;
  }
  return counter % 2 !== 0;
}

/** `GetContinent( pos )` of Madrigal coordinates (`(long)x`, `(long)z`): the first polygon in ascending id order, else `CONT_NODATA`. */
export function continentAt(polys: readonly ContinentPoly[], x: number, z: number): number {
  const px = Math.trunc(x);
  const pz = Math.trunc(z);
  for (const p of polys) if (pointInPoly(p.points, px, pz)) return p.id;
  return CONT_NODATA;
}

/**
 * `CContinent::GetMapNo( pMover )`: the `map` of the `world_maps` row whose `continent` holds the position, only in `WI_WORLD_MADRIGAL`;
 * any other world or continent gives {@link MAP_NONE}.
 */
export function mapNumber(
  worldId: number,
  x: number,
  z: number,
  polys: readonly ContinentPoly[],
  maps: readonly WorldMapRow[],
): number {
  if (worldId !== WI_WORLD_MADRIGAL) return MAP_NONE;
  const cont = continentAt(polys, x, z);
  if (cont === CONT_NODATA) return MAP_NONE;
  const row = maps.find((m) => m.continent === cont);
  return row ? row.map : MAP_NONE;
}
