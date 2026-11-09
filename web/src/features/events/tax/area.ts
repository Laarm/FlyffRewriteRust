// The tax area of the player (`CTax::GetContinent( pMover )` = `GetArea( GetContinent( mover ) )`; `src:_Common/Continent.cpp:257-283, 323-342`, `Tax.cpp:95-163`; Plan/45 §28.3): which of the two tax
// areas, East or West, the character stands in. Pure: the same table as the server's `world/continent.rs` (`continent_of` + `area_of`), so the client prints the rate the server charges.
//
//   world 1 (Madrigal)    the polygon of `WdMadrigal.wld.cnt` that holds `( x, z )`
//   world 208 (arena)     DARKON12 (241)         world 202 (guild war)    FLARIS (1)         any other world    NODATA
//   area                  0 -> NoData; <= 0x0F -> East (15); >= 0xF0 -> West (240); anything between -> NoData

import { WI_WORLD_ARENA, WI_WORLD_GUILDWAR, WI_WORLD_MADRIGAL } from '../../../gen/defs/world';
import { type ContinentPoly, continentAt, CONT_NODATA } from '../../navigator/continent';

export { CONT_NODATA };
/** `CONT_FLARIS`. */
export const CONT_FLARIS = 1;
/** `CONT_EAST`: the eastern tax area. */
export const CONT_EAST = 15;
/** `CONT_WEST`: the western tax area. */
export const CONT_WEST = 240;
/** `CONT_DARKON12`. */
export const CONT_DARKON12 = 241;
/** `CONT_ALL`: the lord's share. */
export const CONT_ALL = 255;

/** `CContinent::GetContinent( mover )`: Madrigal is the polygon test, the arena DARKON12, the guild-war world FLARIS. */
export function continentOfMover(
  worldId: number,
  x: number,
  z: number,
  polys: readonly ContinentPoly[],
): number {
  switch (worldId) {
    case WI_WORLD_MADRIGAL:
      return continentAt(polys, x, z);
    case WI_WORLD_ARENA:
      return CONT_DARKON12;
    case WI_WORLD_GUILDWAR:
      return CONT_FLARIS;
    default:
      return CONT_NODATA;
  }
}

/** `GetArea( cont )` (`__USING_CONTINENT_DATA`): `NODATA` stays, `<= EAST` is `EAST`, `>= WEST` is `WEST`, between is `NODATA`. */
export function areaOf(cont: number): number {
  if (cont === CONT_NODATA) return CONT_NODATA;
  if (cont <= CONT_EAST) return CONT_EAST;
  if (cont >= CONT_WEST) return CONT_WEST;
  return CONT_NODATA;
}

/** The tax area of a position: `GetArea( GetContinent( mover ) )`. */
export function taxAreaOf(
  worldId: number,
  x: number,
  z: number,
  polys: readonly ContinentPoly[],
): number {
  return areaOf(continentOfMover(worldId, x, z, polys));
}
