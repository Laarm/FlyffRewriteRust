// Picking (Plan/43 §5; PL-120; WP-INPUT-06): the pick ray of the displayed camera, the object candidates of the last drawn
// frame with their filters, the closest verified hit, `selectObj` and the ground pick of click-to-move.
//
// Objects are presentation (I4): the candidates are `render.visibleObjects()` (the pick proxies of 41: a model box in the
// model space of the logical placement), the precise refinement is `render.raycastPrecise()` on the current pose. The ground
// is NOT a render question: it is one `SimWorld.ray_cast` (terrain + static collision meshes, §5.5), counted in the Q budget.

import { AII_EGG, AII_PET, RANK_BOSS, RANK_MATERIAL, RANK_MIDBOSS, RANK_SUPER } from '../gen/defs';
import type { PickProxy, Ray3, Vec3LH } from '../render/types';
import type { RayHit } from '../game/ports';

/** `RAY_TERRAIN | RAY_STATIC` (Plan/31 §10.11). */
export const RAY_TERRAIN = 1;
export const RAY_STATIC = 2;
export const RAY_CTRL = 4;
/** `max_t = farPlane / 2` (`World3D.cpp:2366`). */
export const GROUND_PICK_RANGE = 256;

/** What the pick ray needs of the displayed camera (a `CameraState` satisfies it). */
export interface PickCamera {
  readonly pos: Vec3LH;
  readonly lookAt: Vec3LH;
  readonly up: Vec3LH;
  /** `fDiv`: `fovY = pi / fDiv`. */
  readonly fovDiv: number;
}

type V3 = [number, number, number];

const sub = (a: readonly number[], b: readonly number[]): V3 => [
  (a[0] ?? 0) - (b[0] ?? 0),
  (a[1] ?? 0) - (b[1] ?? 0),
  (a[2] ?? 0) - (b[2] ?? 0),
];
const cross = (a: V3, b: V3): V3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const normalize = (v: V3): V3 => {
  const l = Math.hypot(v[0], v[1], v[2]);
  return l > 0 ? [v[0] / l, v[1] / l, v[2] / l] : [0, 0, 0];
};

/**
 * The pick ray through the CSS pixel `(x, y)` of a `w x h` viewport (§5.1, `vutil.cpp:591`), LH game space, direction
 * normalised: `v = ((2x/w - 1) / p11, -(2y/h - 1) / p22, 1)`, `dir = v * invView3x3`, `orig = eye`. With the D3D look-at basis
 * (`LookAtLH`) `invView3x3` maps the view-space axes to the camera's `x`, `y`, `z` axes.
 */
export function pickRayLH(cam: PickCamera, x: number, y: number, w: number, h: number): Ray3 {
  const z = normalize(sub(cam.lookAt, cam.pos));
  const xAxis = normalize(cross(cam.up as V3, z));
  const yAxis = cross(z, xAxis);
  const tanHalf = Math.tan(Math.PI / cam.fovDiv / 2);
  const p22 = 1 / tanHalf;
  const p11 = p22 / (w / h);
  const vx = (2 * (x / w) - 1) / p11;
  const vy = -(2 * (y / h) - 1) / p22;
  const dir = normalize([
    vx * xAxis[0] + vy * yAxis[0] + z[0],
    vx * xAxis[1] + vy * yAxis[1] + z[1],
    vx * xAxis[2] + vy * yAxis[2] + z[2],
  ]);
  return { origin: [cam.pos[0], cam.pos[1], cam.pos[2]], dir };
}

/** `(x, y, z) -> (x, y, -z)`: an LH ray into three.js space (`render/space.ts` `rayToLh` is its own inverse). */
export function rayToThree(r: Ray3): Ray3 {
  return {
    origin: [r.origin[0], r.origin[1], -r.origin[2]],
    dir: [r.dir[0], r.dir[1], -r.dir[2]],
  };
}

/**
 * The parameter `t >= 0` at which `ray` (three.js space) enters the model box of `proxy` (the box lives in the model space
 * of `proxy.worldMatrix`, so the ray is moved there with the inverse of the affine matrix and tested against the slabs;
 * the parameter is preserved). `null` for a miss or a singular matrix.
 */
export function rayVsProxyBox(
  ray: Ray3,
  proxy: Pick<PickProxy, 'worldMatrix' | 'localAabb'>,
): number | null {
  const e = proxy.worldMatrix.elements;
  // column-major: columns 0..2 are the basis, column 3 the translation
  const a00 = e[0];
  const a10 = e[1];
  const a20 = e[2];
  const a01 = e[4];
  const a11 = e[5];
  const a21 = e[6];
  const a02 = e[8];
  const a12 = e[9];
  const a22 = e[10];
  const tx = e[12];
  const ty = e[13];
  const tz = e[14];
  const det =
    a00 * (a11 * a22 - a12 * a21) - a01 * (a10 * a22 - a12 * a20) + a02 * (a10 * a21 - a11 * a20);
  if (!Number.isFinite(det) || Math.abs(det) < 1e-12) return null;
  const id = 1 / det;
  const i00 = (a11 * a22 - a12 * a21) * id;
  const i01 = (a02 * a21 - a01 * a22) * id;
  const i02 = (a01 * a12 - a02 * a11) * id;
  const i10 = (a12 * a20 - a10 * a22) * id;
  const i11 = (a00 * a22 - a02 * a20) * id;
  const i12 = (a02 * a10 - a00 * a12) * id;
  const i20 = (a10 * a21 - a11 * a20) * id;
  const i21 = (a01 * a20 - a00 * a21) * id;
  const i22 = (a00 * a11 - a01 * a10) * id;
  const px = ray.origin[0] - tx;
  const py = ray.origin[1] - ty;
  const pz = ray.origin[2] - tz;
  const o: V3 = [
    i00 * px + i01 * py + i02 * pz,
    i10 * px + i11 * py + i12 * pz,
    i20 * px + i21 * py + i22 * pz,
  ];
  const dx = ray.dir[0];
  const dy = ray.dir[1];
  const dz = ray.dir[2];
  const d: V3 = [
    i00 * dx + i01 * dy + i02 * dz,
    i10 * dx + i11 * dy + i12 * dz,
    i20 * dx + i21 * dy + i22 * dz,
  ];
  const [mn, mx] = proxy.localAabb;
  let t0 = 0;
  let t1 = Number.POSITIVE_INFINITY;
  for (let k = 0; k < 3; k += 1) {
    const ok = o[k] ?? 0;
    const dk = d[k] ?? 0;
    const lo = mn[k] ?? 0;
    const hi = mx[k] ?? 0;
    if (Math.abs(dk) < 1e-12) {
      if (ok < lo || ok > hi) return null;
      continue;
    }
    let a = (lo - ok) / dk;
    let b = (hi - ok) / dk;
    if (a > b) [a, b] = [b, a];
    if (a > t0) t0 = a;
    if (b < t1) t1 = b;
    if (t0 > t1) return null;
  }
  return t0;
}

/** The candidate filters of `PickObject_Fast` (§5.2) that depend on the viewer and the options. */
export interface PickFilter {
  /** The own player is never a candidate. */
  readonly selfId: number;
  /** Tab is held: players are ignored. */
  readonly onlyNpc: boolean;
  /** `features.hide_pets`. */
  readonly hidePetsFeature: boolean;
  /** `input.offTarget || render.hidePets` (the `offPets` alias, §5.2). */
  readonly offTargetOrPets: boolean;
  /** The own loot pet (`ai == AII_PET` and it is the viewer's). */
  readonly ownPetId?: number;
}

/** `true` when the mover/item/ctrl proxy is a candidate under the filter. */
export function isCandidate(p: PickProxy, f: PickFilter): boolean {
  if (p.id === f.selfId) return false;
  if (p.kind === 'mover') {
    if (f.onlyNpc && p.isPlayer) return false; // `World3D.cpp:2700`
    if (p.dead && !p.isPlayer) return false; // `:2706`: dead players stay pickable (resurrection)
    if (f.hidePetsFeature) {
      if (p.ai === AII_EGG && f.offTargetOrPets) return false; // `:2712`
      if (p.ai === AII_PET && p.id !== f.ownPetId && !p.ownPet) return false; // `:2717`
      if (p.ai === AII_PET && (p.id === f.ownPetId || p.ownPet) && f.offTargetOrPets) return false;
    }
  }
  return true;
}

/** The ranks that are re-picked precisely (`RANK_MIDBOSS`, `RANK_SUPER`, `RANK_BOSS`, `RANK_MATERIAL`). */
export function needsPrecise(p: PickProxy): boolean {
  if (p.kind === 'ctrl') return true;
  return (
    p.kind === 'mover' &&
    (p.rank === RANK_MIDBOSS ||
      p.rank === RANK_SUPER ||
      p.rank === RANK_BOSS ||
      p.rank === RANK_MATERIAL)
  );
}

export interface PickHit {
  readonly id: number;
  readonly kind: PickProxy['kind'];
  readonly t: number;
  readonly proxy: PickProxy;
}

/**
 * The closest verified hit (§5.3, PL-120): every candidate whose box the ray meets, nearest first (ties: the lower
 * `ObjId`); controls and the big ranks must also pass the precise triangle test.
 */
export function pickObject(
  proxies: readonly PickProxy[],
  ray: Ray3,
  filter: PickFilter,
  precise: (id: number, ray: Ray3) => number | null,
): PickHit | null {
  const hits: PickHit[] = [];
  for (const p of proxies) {
    if (!isCandidate(p, filter)) continue;
    const t = rayVsProxyBox(ray, p);
    if (t !== null) hits.push({ id: p.id, kind: p.kind, t, proxy: p });
  }
  hits.sort((a, b) => a.t - b.t || a.id - b.id);
  for (const h of hits) {
    if (needsPrecise(h.proxy) && precise(h.id, ray) === null) continue;
    // `GCWAR_NOT_CLICK_MODE`: the pick returns nothing at all
    if (h.proxy.gcwarNotClick) return null;
    return h;
  }
  return null;
}

/** A ground pick: the point and the unit normal of the hit triangle (LH). */
export interface GroundPick {
  readonly pos: readonly [number, number, number];
  readonly normal: readonly [number, number, number];
  readonly kind: number;
}

/** The sim query the ground pick uses (`SimBridge.rayCast`). */
export type RayCaster = (
  origin: Vec3LH,
  dir: Vec3LH,
  maxT: number,
  mask: number,
) => RayHit | null | undefined;

/**
 * `groundPick` (§5.5): one `ray_cast([origin, dir, 256, TERRAIN | STATIC])`. `undefined` = no query slot this iteration
 * (the caller retries), `null` = nothing hit or the hit faces away from the ray (a terrain triangle seen from below,
 * `__FIX_PICKING`: `dot(n, dir) > 0` rejects).
 */
export function groundPick(cast: RayCaster, rayLh: Ray3): GroundPick | null | undefined {
  const hit = cast(rayLh.origin, rayLh.dir, GROUND_PICK_RANGE, RAY_TERRAIN | RAY_STATIC);
  if (hit === undefined) return undefined;
  if (hit === null) return null;
  const n = hit.normal;
  const d = rayLh.dir;
  if (n[0] * d[0] + n[1] * d[1] + n[2] * d[2] > 0) return null;
  return { pos: hit.pos, normal: n, kind: hit.kind };
}
