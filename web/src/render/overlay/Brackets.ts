// Target brackets (Plan/41 section 19.7; WndWorld.cpp:3264-3350 `RenderFocusObj`): four corner sprites around the projected
// bound rectangle of the focus object, and around the queued next target while an auto-attack runs.
//
//   ground   pt1 = (left - 32, top - 32)   pt2 = (right, top - 32)   pt3 = (left - 32, bottom)   pt4 = (right, bottom)
//   flying   pt1 = (left - 16, top - 32)   pt2 = (right - 16, top - 32)   pt3 = (left - 16, bottom)   pt4 = (right - 16, bottom)
//
// The sprites are `Icon/icon_target.inc` (ground) or `Icon/icon_FlightTargetB.inc` (the own player flies). Both hold ONE corner
// picture, the upper-left arrow, in indices 0 (white) and 8 (red). The original draws indices 0..3 / 8..11 and lets the .inc flags
// `h` / `v` / `hv` of 1..3 / 9..11 mirror that picture for the other three corners. BFIX1 #001 (owner's decision): the texture path
// of the overlay never carried the flag, so all four corners were the same arrow; the owner asks for ROTATIONS instead of mirrors,
// so indices 1..3 and 9..11 are not used: every corner is the base sprite (`index: first`) turned clockwise by `CORNER_TURNS`
// (TL 0, TR 90, BL 270, BR 180) about its own centre and so points at the centre of the target. Rotation and mirroring differ only in
// which side of the arrow carries the highlight.
// The focus is red when it is a hostile mover (or a player) the own player is engaged with (`TargetState.red`); the next target is
// always white (`pObj != m_pNextTargetObj` is false for it). Flight target rectangles and the off-screen arrow of a flying player
// are WP-REND-25.

import type { ObjId, ScreenRect, TargetState } from '../types';
import type { SpriteItem } from './types';

export const TARGET_ATLAS = 'icon/icon_target.inc';
export const TARGET_FLY_ATLAS = 'icon/icon_flighttargetb.inc';

/** Sprite index of the white corner picture / of the red one (the upper-left arrow of the atlas). */
export const WHITE_CORNERS = 0;
export const RED_CORNERS = 8;

/** Clockwise turns of the corner picture in `bracketPositions` order: top-left, top-right, bottom-left, bottom-right (BFIX1 #001). */
export const CORNER_TURNS = [0, 90, 270, 180] as const;

export type Point = readonly [number, number];

/** The four corner positions (top-left, top-right, bottom-left, bottom-right) of the bracket sprites. */
export function bracketPositions(
  rect: ScreenRect,
  flying: boolean,
): readonly [Point, Point, Point, Point] {
  const left = rect.x;
  const top = rect.y;
  const right = rect.x + rect.w;
  const bottom = rect.y + rect.h;
  return flying
    ? [
        [left - 16, top - 32],
        [right - 16, top - 32],
        [left - 16, bottom],
        [right - 16, bottom],
      ]
    : [
        [left - 32, top - 32],
        [right, top - 32],
        [left - 32, bottom],
        [right, bottom],
      ];
}

/** The four corner sprites around `rect`. Positions are truncated to whole pixels like the original's `CPoint`. */
export function layoutBrackets(rect: ScreenRect, flying: boolean, red: boolean): SpriteItem[] {
  const atlas = flying ? TARGET_FLY_ATLAS : TARGET_ATLAS;
  const first = red ? RED_CORNERS : WHITE_CORNERS;
  return bracketPositions(rect, flying).map(([x, y], i) => ({
    kind: 'sprite',
    atlas,
    index: first,
    x: Math.trunc(x),
    y: Math.trunc(y),
    alpha: 255,
    rotate: CORNER_TURNS[i] ?? 0,
  }));
}

/** What the brackets of a frame need: the target state and the projected bound rect of an object (`null` = not on screen). */
export interface BracketInput {
  readonly targets: TargetState | null;
  readonly flying: boolean;
  readonly boundsOf: (id: ObjId) => ScreenRect | null;
}

/**
 * The bracket sprites of a frame: the focus (red when `targets.red`), then the queued next target (white). An object that has no
 * bound rect this frame (culled, behind the camera, not drawn) gets none.
 */
export function layoutTargetBrackets(input: BracketInput): SpriteItem[] {
  const t = input.targets;
  if (!t) return [];
  const out: SpriteItem[] = [];
  if (t.focus !== 0) {
    const r = input.boundsOf(t.focus);
    if (r) out.push(...layoutBrackets(r, input.flying, t.red));
  }
  if (t.next !== 0 && t.next !== t.focus) {
    const r = input.boundsOf(t.next);
    if (r) out.push(...layoutBrackets(r, input.flying, false));
  }
  return out;
}
