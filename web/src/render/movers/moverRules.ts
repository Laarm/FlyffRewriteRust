// Per-mover render rules of the world (Plan/41 section 12.8, ref/21 section 6.3.2 and 6.8; `CMover::Render`,
// MoverRender.cpp:165-720): PURE decisions the mover batcher applies every frame, kept free of three.js and of the scene so
// that the tables of the plan are unit-testable:
//
//   * the far cut-off (3/4 of the far plane = 384 m), the draw sphere of a mover (`moverCullSphere`) and the pose window,
//   * who draws what: `TRANSPARENT_MODE` (GM invisibility), the render-skip modes, hidden pets, the telescope,
//   * the LOD group (`lodFor`, shared with `MoverVisual`), the highlight rule, the blob-shadow rule,
//   * hit shake (`dmgCnt`), the special-case models (`MI_SYLIACA1..4`, `MI_MASA_TINA`), `want_attach` of `SET_VISIBLE`.

import { RANK_SUPER } from '../../gen/defs/attribute';
import { AII_EGG, AII_PET } from '../../gen/defs/neuz';
import {
  MI_MASA_TINA,
  MI_SYLIACA1,
  MI_SYLIACA2,
  MI_SYLIACA3,
  MI_SYLIACA4,
} from '../../gen/defs/obj';
import { BOSS_MODEL_IDS } from '../../game/fx/bossIds';
import type { PartElement } from './partRules';

/** `farPlane * 3 / 4`: movers farther than this from the camera are not drawn (`nLength > 384`, MoverRender.cpp:315-321). */
export const FAR_CUT_M = 384;
/** Smallest margin added to the radius of the draw sphere (Plan/41 12.8 "Sim visibility"). */
export const FRUSTUM_MARGIN_M = 2;
/** Smallest sim radius the draw sphere works with (a mover whose bounds are not known yet has radius 0). */
export const MIN_SPHERE_RADIUS_M = 0.5;
/**
 * The part of the sphere radius added on top of the model box for what the box does not hold: attack swings, wings, a weapon
 * held out (#018 B4). The margin is `max(FRUSTUM_MARGIN_M, ANIM_MARGIN_FRACTION * r)`.
 */
export const ANIM_MARGIN_FRACTION = 0.5;
/**
 * Half the side of the square around the active mover inside which every mover keeps its visual and its pose, on screen or not
 * (#018 B5): the server's view window. With `n_view` 1 the window radius is 2 cells of 32 m
 * (`flyff-game` `world/catalog.rs`, `grid.rs`), so a mover the server keeps is at most 3 cells = 96 m away per axis.
 */
export const POSE_WINDOW_M = 96;
/** Beyond the pose window a mover is posed (`SET_VISIBLE`) as soon as the frustum reaches within this distance of its draw sphere. */
export const POSE_LOOKAHEAD_M = 8;
/** How long `SET_VISIBLE` stays on after a mover beyond the pose window left the look-ahead (no toggling at an edge). */
export const VISIBLE_GRACE_MS = 1000;
/** A mover beyond the pose window that was not wanted for this long gives its visual back (it was 15 s before #018). */
export const MOVER_KEEP_ALIVE_MS = 60_000;

/** `m_dwMode` bits of `authorization.h` that the renderer reads (Plan/16 section 2.5, Plan/23 section 2.3). */
export const MODE = {
  /** `TRANSPARENT_MODE`: GM invisibility (`/inv`). */
  TRANSPARENT: 0x0000_0002,
  /** `GCWAR_RENDER_SKIP_MODE`: guild-siege participants of the other side. */
  GCWAR_RENDER_SKIP: 0x0010_0000,
  /** `QUIZ_RENDER_SKIP_MODE`. */
  QUIZ_RENDER_SKIP: 0x0020_0000,
} as const;

/** `AUTH_GAMEMASTER` ('L'): the first authority that sees invisible movers; authorities compare as numbers (Plan/23 section 1.1). */
export const AUTH_GAMEMASTER = 76;

/** The blend factor of a mover in `TRANSPARENT_MODE` that is still drawn (80 / 255). */
export const TRANSPARENT_BLEND = 80;

/** Why a mover is not drawn. */
export type DrawVerdict =
  'draw' | 'far' | 'transparent' | 'renderSkip' | 'pet' | 'telescope' | 'noModel';

/** What `decideDraw` reads of one mover and of the viewer. */
export interface DrawInput {
  /** The own (active) mover. */
  readonly isActive: boolean;
  /** `m_dwMode` of the mover. */
  readonly mode: number;
  /** The authority byte of the VIEWER (the own player), `0` when unknown. */
  readonly viewerAuthority: number;
  /** `MoverProp.dwAI` (`AII_PET` / `AII_EGG` pets are hidden by the option). */
  readonly ai: number;
  /** `RenderOptions.hidePets`. */
  readonly hidePets: boolean;
  /** `CameraState.telescope`: the own model is hidden while zoomed (not 0). */
  readonly telescope: number;
  /** Distance camera -> logical position (`m_fDistCamera`). */
  readonly dist: number;
}

/** The result of `decideDraw`. */
export interface DrawDecision {
  readonly draw: boolean;
  readonly verdict: DrawVerdict;
}

/** `true` for the authorities that see GM-invisible movers (`>= AUTH_GAMEMASTER`). */
export function seesInvisible(viewerAuthority: number): boolean {
  return viewerAuthority >= AUTH_GAMEMASTER;
}

/** `true` for the movers the "hide pets" option hides (`AII_PET`, `AII_EGG`). */
export function isHiddenPet(ai: number, hidePets: boolean): boolean {
  return hidePets && (ai === AII_PET || ai === AII_EGG);
}

/**
 * Whether a mover is drawn at all (Plan/41 12.8 "Visibility" and "TRANSPARENT_MODE"): the first matching row wins.
 *
 *  1. the own mover is hidden while the camera zooms (`telescope != 0`),
 *  2. a mover beyond the far cut-off (`dist > 384`) is not drawn (the own mover is never that far),
 *  3. `TRANSPARENT_MODE`: drawn (with blend 80) only for the active mover and for a viewer of authority >= GAMEMASTER,
 *  4. `GCWAR_RENDER_SKIP` / `QUIZ_RENDER_SKIP` hide OTHER movers,
 *  5. pets are hidden by `hidePets`.
 */
export function decideDraw(i: DrawInput): DrawDecision {
  if (i.isActive && i.telescope !== 0) return { draw: false, verdict: 'telescope' };
  if (!i.isActive && i.dist > FAR_CUT_M) return { draw: false, verdict: 'far' };
  if ((i.mode & MODE.TRANSPARENT) !== 0 && !i.isActive && !seesInvisible(i.viewerAuthority)) {
    return { draw: false, verdict: 'transparent' };
  }
  if (!i.isActive && (i.mode & (MODE.GCWAR_RENDER_SKIP | MODE.QUIZ_RENDER_SKIP)) !== 0) {
    return { draw: false, verdict: 'renderSkip' };
  }
  if (isHiddenPet(i.ai, i.hidePets)) return { draw: false, verdict: 'pet' };
  return { draw: true, verdict: 'draw' };
}

/**
 * `true` when a drawn mover gets name, HP bar, shadow and mouse-over highlight: not a GM-invisible (`TRANSPARENT_MODE`)
 * mover (Plan/41 12.8, Plan/23 section 2.3 `TRANSPARENT` row).
 */
export function hasDecorations(mode: number): boolean {
  return (mode & MODE.TRANSPARENT) === 0;
}

/** The box of a model in model space (three.js): `Box3` satisfies it. */
export interface BoxLike {
  readonly min: { readonly x: number; readonly y: number; readonly z: number };
  readonly max: { readonly x: number; readonly y: number; readonly z: number };
}

/** Where `moverCullSphere` writes: three.js `Sphere` satisfies it. */
export interface SphereOut {
  readonly center: { x: number; y: number; z: number };
  radius: number;
}

/**
 * The sphere a mover is culled with (three.js space; #018 B4): a BOUND of the model, not the sim's `GetRadius` (an average
 * half-extent, `(|dx| + |dy| + |dz|) / 6 * scale`, which misses the corners of giants, wide models and attack swings).
 *
 *  * with the model box (`box`: the union of the loaded parts' boxes, model space, or `null` while empty): the centre is `world`
 *    applied to the box centre, `r` is half the box diagonal times the largest column length of `world`'s 3x3 (the scale), the
 *    radius is `r + max(FRUSTUM_MARGIN_M, ANIM_MARGIN_FRACTION * r)`. The yaw and the box offset are inside the centre.
 *  * without a box: `r0 = max(simRadius, MIN_SPHERE_RADIUS_M)`, the centre is the translation of `world` plus `(0, r0, 0)`,
 *    `r = 3 * r0` (a box whose `GetRadius` is `r0` has a half diagonal of at most 3 times it: its extents add up to 6 `r0`),
 *    the radius is `r + max(FRUSTUM_MARGIN_M, ANIM_MARGIN_FRACTION * r)`.
 *
 * `world` is the 16 column-major elements of the mover's three.js draw matrix (`MoverPoseFrame.world.elements`). Returns `out`.
 */
export function moverCullSphere<S extends SphereOut>(
  box: BoxLike | null,
  world: ArrayLike<number>,
  simRadius: number,
  out: S,
): S {
  const e = (i: number): number => world[i] ?? 0;
  let r: number;
  if (box) {
    const cx = (box.min.x + box.max.x) / 2;
    const cy = (box.min.y + box.max.y) / 2;
    const cz = (box.min.z + box.max.z) / 2;
    out.center.x = e(0) * cx + e(4) * cy + e(8) * cz + e(12);
    out.center.y = e(1) * cx + e(5) * cy + e(9) * cz + e(13);
    out.center.z = e(2) * cx + e(6) * cy + e(10) * cz + e(14);
    const half =
      Math.hypot(box.max.x - box.min.x, box.max.y - box.min.y, box.max.z - box.min.z) / 2;
    const scale = Math.max(
      Math.hypot(e(0), e(1), e(2)),
      Math.hypot(e(4), e(5), e(6)),
      Math.hypot(e(8), e(9), e(10)),
    );
    r = half * scale;
  } else {
    const r0 = Math.max(simRadius, MIN_SPHERE_RADIUS_M);
    out.center.x = e(12);
    out.center.y = e(13) + r0;
    out.center.z = e(14);
    r = 3 * r0;
  }
  out.radius = r + Math.max(FRUSTUM_MARGIN_M, ANIM_MARGIN_FRACTION * r);
  return out;
}

/**
 * `true` when a mover at `pos` is inside the pose window around the active mover (`active`): the square of
 * `POSE_WINDOW_M` in x and z like the server grid; the height does not count. Both are LH positions.
 */
export function inPoseWindow(pos: ArrayLike<number>, active: ArrayLike<number>): boolean {
  return (
    Math.abs((pos[0] ?? 0) - (active[0] ?? 0)) <= POSE_WINDOW_M &&
    Math.abs((pos[2] ?? 0) - (active[2] ?? 0)) <= POSE_WINDOW_M
  );
}

/**
 * The model-blend factor of a mover (`CModelObject::Render` `blendFactor`): `80` in `TRANSPARENT_MODE`, else `fade`.
 * (`MoverVisual.blend` applies the same rule; this is the table the tests assert.)
 */
export function blendOf(mode: number, fade = 255): number {
  return (mode & MODE.TRANSPARENT) !== 0 ? TRANSPARENT_BLEND : fade;
}

// ---- hit shake ---------------------------------------------------------------------------------------------

/** `dmgCnt` set by the damage visual path (Plan/41 12.8: `dmgCnt = 10`). */
export const HIT_SHAKE_TICKS = 10;

/** `true` when a mover shakes: an NPC / monster (not a player) with `dmgCnt > 0` and rank other than `RANK_SUPER`. */
export function shakes(dmgCnt: number, isPlayer: boolean, rank: number): boolean {
  return dmgCnt > 0 && !isPlayer && rank !== RANK_SUPER;
}

/** The shake offset (LH x, z) of one tick: `cx, cz = -25 + rand(50)` hundredths, i.e. -0.25 ..= +0.24 (re-rolled per TICK, FIX). */
export function shakeOffset(rand: () => number): [number, number] {
  const cx = -25 + Math.floor(rand() * 50);
  const cz = -25 + Math.floor(rand() * 50);
  return [cx / 100, cz / 100];
}

// ---- special-case models -----------------------------------------------------------------------------------

/** `MI_SYLIACA1..4`: the model origin is the centre (world `y += |bb.max.y - bb.min.y| / 2`). */
export function isSyliaca(moverIndex: number): boolean {
  return (
    moverIndex === MI_SYLIACA1 ||
    moverIndex === MI_SYLIACA2 ||
    moverIndex === MI_SYLIACA3 ||
    moverIndex === MI_SYLIACA4
  );
}

/** `MI_MASA_TINA`: the server scale is ignored (the model default scale is reset every frame). */
export function ignoresServerScale(moverIndex: number): boolean {
  return moverIndex === MI_MASA_TINA;
}

/** The lift of a `MI_SYLIACA` mover in world units: half the model height (`bbox` extent along y). */
export function syliacaLift(bboxMinY: number, bboxMaxY: number): number {
  return Math.abs(bboxMaxY - bboxMinY) / 2;
}

// ---- attachments -------------------------------------------------------------------------------------------

/**
 * `want_attach` of `SET_VISIBLE` (Plan/41 12.8 "Sim visibility"): the sim only fills the attachment record of a mover when
 * somebody places something on it (weapons, a shield, wings, a ride, active parts effects).
 */
export function wantsAttach(
  elements: readonly PartElement[],
  rideItem: number | null,
  moverIndex = 0,
): boolean {
  // ---- render-audio-m7 (WP-REND-27): the boss models need their event points and bones for the effects (they carry no weapon, so nothing mounts on them)
  if (BOSS_MODEL_SET.has(moverIndex)) return true;
  return rideItem !== null || elements.some((e) => e.mount !== null && !e.hidden);
}

const BOSS_MODEL_SET: ReadonlySet<number> = new Set(BOSS_MODEL_IDS);
