// The floors of the Heaven Tower (`CWndHeavenTowerEntranceConfirm`, `src:_Interface/WndField.cpp:22172-22445`; Plan/45 §28.3; WP-CFEAT-52): the floor guard of the send and the costs the window shows. The window
// itself (the rows, the description, the confirmation with the costs, the floor-6 row that sends nothing) is ui-quest's `WndHeavenTower` shell; the server charges the fee and answers `TID_GAME_LACKMONEY`.

export { HEAVEN_TOWER_COSTS } from '../../../ui/windows/quest/rows';

/** The lowest and highest floor that can be entered (floors B1 .. B5; the sixth row of the window is a floor that sends nothing). */
export const HEAVEN_FLOOR_MIN = 1;
export const HEAVEN_FLOOR_MAX = 5;

/** A floor the server accepts (`1 <= floor <= 5`); anything else is not sent. */
export function isTowerFloor(floor: number): floor is 1 | 2 | 3 | 4 | 5 {
  return Number.isInteger(floor) && floor >= HEAVEN_FLOOR_MIN && floor <= HEAVEN_FLOOR_MAX;
}
