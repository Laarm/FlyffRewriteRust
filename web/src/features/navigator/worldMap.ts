// The geometry of the world-map overlay (`CWorldMap`, `src:Neuz/WorldMap.cpp`; ref/99 §4.5; Plan/45 §7.7; WP-CFEAT-28): the 4:3 box
// letter-boxed in the viewport, the nine continent buttons, the world -> map transform and the teleporter icons. The textures, the world
// rectangles and the continent -> map rule are DATA (`worlds.world_maps`, Plan/14 §10.5): nothing of `WorldMap.cpp` / `Continent.cpp` is
// transcribed here except the transform, which is code in the original too.
//
//   box        the largest 4:3 rect that fits the viewport, centred (the original had one box per resolution)
//   rate       `viewportH / 1024` (`m_fRate`): the scale of the continent buttons
//   buttons    nine 228 x 88 strips of `Theme/texMapButton.inc` (3 frames each: normal, hover, selected) from y = 80 * rate; the first
//              `world_maps.length` select a map, the last one toggles the monster info (M8)
//   transform  nX = (int)(right - (right - left) * (X2 - x) / (X2 - X1)),  nY = (int)(bottom - (bottom - top) * (z - Y1) / (Y2 - Y1))
//              with [X1, Z1, X2, Z2] = `world_maps[map].rect`: linear, z inverted, only for positions on the shown map (map 0 shows all)

import type { WorldMapRow } from '../../gen/bundles';
import { type ContinentPoly, mapNumber } from './continent';

export interface BoxRect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** The map box: the largest 4:3 rect inside the viewport, centred; `rate` is the button scale. */
export function mapBox(viewW: number, viewH: number): { box: BoxRect; rate: number } {
  const h = Math.min(viewH, Math.floor((viewW * 3) / 4));
  const w = Math.floor((h * 4) / 3);
  return {
    box: { x: Math.floor((viewW - w) / 2), y: Math.floor((viewH - h) / 2), w, h },
    rate: viewH / 1024,
  };
}

/** `m_texMapButton` frame size (`texMapButton.inc`: 228 x 88). */
export const BUTTON_W = 228;
export const BUTTON_H = 88;
/** The first button sits at `y = 80 * rate`. */
export const BUTTON_TOP = 80;
/** Nine buttons: eight maps and the monster-info toggle (`MAX_BUTTON`). */
export const BUTTON_COUNT = 9;

export interface ButtonRect {
  readonly index: number;
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** `m_cRect[i]`: the buttons in a column from `y = 80 * rate`, shifted right by the box offset on a wide viewport. */
export function mapButtons(viewW: number, viewH: number, count = BUTTON_COUNT): ButtonRect[] {
  const { box, rate } = mapBox(viewW, viewH);
  const w = Math.trunc(BUTTON_W * rate);
  const h = Math.trunc(BUTTON_H * rate);
  const out: ButtonRect[] = [];
  let y = Math.trunc(BUTTON_TOP * rate);
  for (let i = 0; i < count; i += 1) {
    out.push({ index: i, x: box.x, y, w, h });
    y += h;
  }
  return out;
}

/** The frame of `texMapButton.inc` of button `i`: `3 i`, `+ 1` under the pointer (not for the selected one), `+ 2` for the shown map. */
export function buttonFrame(i: number, shown: number, hover: boolean): number {
  const base = 3 * i;
  if (i === shown) return base + 2;
  return hover ? base + 1 : base;
}

/**
 * `WorldPosToMapPos`: the screen position of a world position on the map `mapNo`, `null` when the position is not on it (for a map other
 * than the whole world only the positions inside its continent show).
 */
export function worldToMap(
  maps: readonly WorldMapRow[],
  mapNo: number,
  worldId: number,
  x: number,
  z: number,
  polys: readonly ContinentPoly[],
  box: BoxRect,
): { x: number; y: number } | null {
  const row = maps.find((m) => m.map === mapNo);
  if (!row) return null;
  if (mapNo !== 0 && mapNumber(worldId, x, z, polys, maps) !== mapNo) return null;
  const [x1, z1, x2, z2] = row.rect;
  const rate1 = (x2 - x) / (x2 - x1);
  const rate2 = (z - z1) / (z2 - z1);
  const right = box.x + box.w;
  const bottom = box.y + box.h;
  return {
    x: Math.trunc(right - (right - box.x) * rate1),
    y: Math.trunc(bottom - (bottom - box.y) * rate2),
  };
}

/** The teleporter / destination icon edge (`CSize( 16, 16 )`) and the hovered size (20 x 20 at `-2`). */
export const ICON_SIZE = 16;
export const ICON_HOVER_SIZE = 20;

export interface TeleportIcon {
  /** The index in the NPC's teleport list (what `Teleporter{key, index}` sends). */
  readonly index: number;
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** `UpdateTeleportWorld`: one icon per destination that is on the shown map, centred on the destination (`(int)(pt - 8)`). */
export function teleportIcons(
  positions: readonly (readonly [number, number])[],
  maps: readonly WorldMapRow[],
  mapNo: number,
  worldId: number,
  polys: readonly ContinentPoly[],
  box: BoxRect,
): TeleportIcon[] {
  const out: TeleportIcon[] = [];
  positions.forEach((p, index) => {
    const at = worldToMap(maps, mapNo, worldId, p[0], p[1], polys, box);
    if (!at) return;
    out.push({
      index,
      x: Math.trunc(at.x - ICON_SIZE * 0.5),
      y: Math.trunc(at.y - ICON_SIZE * 0.5),
      w: ICON_SIZE,
      h: ICON_SIZE,
    });
  });
  return out;
}

/** The icon under the pointer, the first in list order (`OnLButtonDown` walks `m_cTelPosRects` from the front). */
export function iconAt(icons: readonly TeleportIcon[], x: number, y: number): TeleportIcon | null {
  for (const i of icons) {
    if (x >= i.x && x < i.x + i.w && y >= i.y && y < i.y + i.h) return i;
  }
  return null;
}

/** The button under the pointer. */
export function buttonAt(buttons: readonly ButtonRect[], x: number, y: number): ButtonRect | null {
  for (const b of buttons) {
    if (x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h) return b;
  }
  return null;
}

/** The size of the player arrows: `0.07` of the viewport height (the billboard of `RenderPlayer` is scaled by 0.07 of the clip space). */
export function arrowSize(viewH: number): number {
  return Math.max(16, Math.round(viewH * 0.07));
}
