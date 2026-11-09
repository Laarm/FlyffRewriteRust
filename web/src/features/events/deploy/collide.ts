// The collision test of a placed furniture (`CWorld::ProcessObjCollision`, `TestOBBIntersect`, `TestTriIntersect`, `CObject3D::SimpleTriIntersect`, `src:_Common/WorldIntersect.cpp:64-210`, `Object3D.cpp:2133-2290`; Plan/45 §28.10, Plan/43
// §7.13; WP-CFEAT-46): pure math in the LEFT-HANDED world space of the server, matrices in the D3DX row-vector form (16 numbers, `v' = v * M`). Nothing here knows the renderer: the ghost renderer hands the scene over
// (`DeployScene`).
//
//   housing   no wall object                                            -> never red (KEEP `CheckCollision`)
//             ghost OBB does not overlap the wall OBB                   -> red (outside the room)
//             a triangle of the ghost crosses a triangle of the wall    -> red
//             an OBB of another ctrl overlaps the ghost OBB             -> red
//   guild     first the height attribute under the ghost                -> NOWALK 1 / NOFLY 2 / NOMOVE 3 are red
//
// FIX / simplification against the original: ALL ctrls in view are tested (the original walked the link map around the ghost); `exceptObjId` is the piece a reinstall moves (the original skipped
// `GuildHouse->m_dwSelectedObjID`). The triangle test is Moeller's, correct for coplanar triangles too (the original's hand-rolled version used a wrong axis for a y-dominant intersection line).

import type { DeployBody, DeployMesh, DeployScene } from '../../../game/ports';
import type { Vec3LH } from '../../../render/types';

export type { DeployBody, DeployMesh, DeployScene };

/** A 4x4 matrix in the D3DX memory order (`_11 _12 _13 _14 _21 ...`), row vectors. */
export type Mat16 = ArrayLike<number>;

/** `HATTR_NOWALK`, `HATTR_NOFLY`, `HATTR_NOMOVE` of the height attribute. */
export const HATTR_NOWALK = 1;
export const HATTR_NOFLY = 2;
export const HATTR_NOMOVE = 3;

/** The oriented box of the original (`BBOX`: `Center`, `Extent[3]`, `Axis[3]`). */
export interface Obb {
  readonly center: Vec3LH;
  readonly extent: Vec3LH;
  readonly axis: readonly [Vec3LH, Vec3LH, Vec3LH];
}

const at = (m: Mat16, i: number): number => m[i] ?? 0;

/** `v * M` of a point (the w row added). */
export function transformCoord(v: Vec3LH, m: Mat16): Vec3LH {
  return [
    v[0] * at(m, 0) + v[1] * at(m, 4) + v[2] * at(m, 8) + at(m, 12),
    v[0] * at(m, 1) + v[1] * at(m, 5) + v[2] * at(m, 9) + at(m, 13),
    v[0] * at(m, 2) + v[1] * at(m, 6) + v[2] * at(m, 10) + at(m, 14),
  ];
}

/** The world matrix of a ghost: `Scale( 1 ) * RotYawPitchRoll( -angle degrees, 0, 0 ) * Translate( pos )` (D3DX row-vector form). */
export function ghostMatrix(pos: Vec3LH, angleDeg: number): number[] {
  const yaw = (-angleDeg * Math.PI) / 180;
  const c = Math.cos(yaw);
  const s = Math.sin(yaw);
  return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, pos[0], pos[1], pos[2], 1];
}

/**
 * The world OBB of a model box under `world = S * R * T` (`BBOX::UpdateMartix`, mirrored by `flyff-sim/src/collision/dynamic.rs::obb_of`): the centre is the transformed box centre, the half extents scale
 * with the row lengths, axis `i` is row `i` normalised. A box stored with `min > max` on an axis gives a negative extent that is made positive (`__BS_BBOX_ABS_EXTENT`).
 */
export function obbOf(world: Mat16, bbox: { readonly min: Vec3LH; readonly max: Vec3LH }): Obb {
  const row = (r: number): Vec3LH => [at(world, r * 4), at(world, r * 4 + 1), at(world, r * 4 + 2)];
  const len = (v: Vec3LH): number => Math.hypot(v[0], v[1], v[2]);
  const centerLocal: Vec3LH = [
    (bbox.min[0] + bbox.max[0]) / 2,
    (bbox.min[1] + bbox.max[1]) / 2,
    (bbox.min[2] + bbox.max[2]) / 2,
  ];
  const half: Vec3LH = [
    Math.abs(bbox.max[0] - bbox.min[0]) / 2,
    Math.abs(bbox.max[1] - bbox.min[1]) / 2,
    Math.abs(bbox.max[2] - bbox.min[2]) / 2,
  ];
  const rows = [row(0), row(1), row(2)] as const;
  const s = [len(rows[0]), len(rows[1]), len(rows[2])] as const;
  const unit = (r: Vec3LH, l: number): Vec3LH =>
    l > 0 ? [r[0] / l, r[1] / l, r[2] / l] : [0, 0, 0];
  return {
    center: transformCoord(centerLocal, world),
    extent: [half[0] * s[0], half[1] * s[1], half[2] * s[2]],
    axis: [unit(rows[0], s[0]), unit(rows[1], s[1]), unit(rows[2], s[2])],
  };
}

const dot = (a: Vec3LH, b: Vec3LH): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/**
 * `CWorld::TestOBBIntersect`: 15-axis separating-axis test, the terms of the original line for line (`t > ra + rb` separates). Touching boxes overlap.
 */
export function testObbIntersect(a: Obb, b: Obb): boolean {
  const vDist: Vec3LH = [
    b.center[0] - a.center[0],
    b.center[1] - a.center[1],
    b.center[2] - a.center[2],
  ];
  const ea = a.extent;
  const eb = b.extent;
  const axesA = a.axis;
  const axesB = b.axis;
  const R: number[] = [];
  const absR: number[] = [];
  for (const axA of axesA) {
    for (const axB of axesB) {
      const v = dot(axA, axB);
      R.push(v);
      absR.push(Math.abs(v));
    }
  }
  const t = [dot(vDist, a.axis[0]), dot(vDist, a.axis[1]), dot(vDist, a.axis[2])] as const;
  const r = (i: number, k: number): number => R[i * 3 + k] ?? 0;
  const ar = (i: number, k: number): number => absR[i * 3 + k] ?? 0;
  const e = (v: Vec3LH, i: number): number => v[i] ?? 0;
  // the three axes of A
  for (let i = 0; i < 3; i += 1) {
    const ra = e(ea, i);
    const rb = e(eb, 0) * ar(i, 0) + e(eb, 1) * ar(i, 1) + e(eb, 2) * ar(i, 2);
    if (Math.abs(t[i] ?? 0) > ra + rb) return false;
  }
  // the three axes of B
  for (let k = 0; k < 3; k += 1) {
    const ra = e(ea, 0) * ar(0, k) + e(ea, 1) * ar(1, k) + e(ea, 2) * ar(2, k);
    const rb = e(eb, k);
    const tt = Math.abs(t[0] * r(0, k) + t[1] * r(1, k) + t[2] * r(2, k));
    if (tt > ra + rb) return false;
  }
  // the nine cross products
  const cross = (
    raTerms: readonly [number, number, number, number],
    rbTerms: readonly [number, number, number, number],
    tTerm: number,
  ): boolean => {
    const ra = raTerms[0] * raTerms[1] + raTerms[2] * raTerms[3];
    const rb = rbTerms[0] * rbTerms[1] + rbTerms[2] * rbTerms[3];
    return Math.abs(tTerm) > ra + rb;
  };
  // L = A0 x B0 .. A2 x B2 (indices as in the original, `fReal[i][k] = A_i . B_k`)
  if (
    cross(
      [e(ea, 1), ar(2, 0), e(ea, 2), ar(1, 0)],
      [e(eb, 1), ar(0, 2), e(eb, 2), ar(0, 1)],
      t[2] * r(1, 0) - t[1] * r(2, 0),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 1), ar(2, 1), e(ea, 2), ar(1, 1)],
      [e(eb, 0), ar(0, 2), e(eb, 2), ar(0, 0)],
      t[2] * r(1, 1) - t[1] * r(2, 1),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 1), ar(2, 2), e(ea, 2), ar(1, 2)],
      [e(eb, 0), ar(0, 1), e(eb, 1), ar(0, 0)],
      t[2] * r(1, 2) - t[1] * r(2, 2),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 0), ar(2, 0), e(ea, 2), ar(0, 0)],
      [e(eb, 1), ar(1, 2), e(eb, 2), ar(1, 1)],
      t[0] * r(2, 0) - t[2] * r(0, 0),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 0), ar(2, 1), e(ea, 2), ar(0, 1)],
      [e(eb, 0), ar(1, 2), e(eb, 2), ar(1, 0)],
      t[0] * r(2, 1) - t[2] * r(0, 1),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 0), ar(2, 2), e(ea, 2), ar(0, 2)],
      [e(eb, 0), ar(1, 1), e(eb, 1), ar(1, 0)],
      t[0] * r(2, 2) - t[2] * r(0, 2),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 0), ar(1, 0), e(ea, 1), ar(0, 0)],
      [e(eb, 1), ar(2, 2), e(eb, 2), ar(2, 1)],
      t[1] * r(0, 0) - t[0] * r(1, 0),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 0), ar(1, 1), e(ea, 1), ar(0, 1)],
      [e(eb, 0), ar(2, 2), e(eb, 2), ar(2, 0)],
      t[1] * r(0, 1) - t[0] * r(1, 1),
    )
  )
    return false;
  if (
    cross(
      [e(ea, 0), ar(1, 2), e(ea, 1), ar(0, 2)],
      [e(eb, 0), ar(2, 1), e(eb, 1), ar(2, 0)],
      t[1] * r(0, 2) - t[0] * r(1, 2),
    )
  )
    return false;
  return true;
}

// ---- triangles ---------------------------------------------------------------------------------------------------------------------

export type Tri = readonly [Vec3LH, Vec3LH, Vec3LH];

const sub = (a: Vec3LH, b: Vec3LH): Vec3LH => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross3 = (a: Vec3LH, b: Vec3LH): Vec3LH => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

const EPS = 1e-6;

/** Whether the point `p` of the plane of `tri` lies inside it (barycentric signs, the plane normal `n`). */
function pointInTri(p: Vec3LH, tri: Tri, n: Vec3LH): boolean {
  const s0 = dot(cross3(sub(tri[1], tri[0]), sub(p, tri[0])), n);
  const s1 = dot(cross3(sub(tri[2], tri[1]), sub(p, tri[1])), n);
  const s2 = dot(cross3(sub(tri[0], tri[2]), sub(p, tri[2])), n);
  return (s0 >= -EPS && s1 >= -EPS && s2 >= -EPS) || (s0 <= EPS && s1 <= EPS && s2 <= EPS);
}

/** 2-D orientation of `c` against the directed line `a -> b` after dropping the coordinate `drop` of the three points. */
function orient(a: Vec3LH, b: Vec3LH, c: Vec3LH, drop: number): number {
  const u = drop === 0 ? 1 : 0;
  const v = drop === 2 ? 1 : 2;
  return (b[u] - a[u]) * (c[v] - a[v]) - (b[v] - a[v]) * (c[u] - a[u]);
}

/** Two segments of one plane cross (touching and collinear overlap included); the plane is projected along its dominant normal axis. */
function segmentsCross(p1: Vec3LH, p2: Vec3LH, q1: Vec3LH, q2: Vec3LH, n: Vec3LH): boolean {
  let drop = 0;
  if (Math.abs(n[1]) > Math.abs(n[0])) drop = 1;
  if (Math.abs(n[2]) > Math.abs(n[drop] ?? 0)) drop = 2;
  const u = drop === 0 ? 1 : 0;
  const v = drop === 2 ? 1 : 2;
  const o1 = orient(p1, p2, q1, drop);
  const o2 = orient(p1, p2, q2, drop);
  const o3 = orient(q1, q2, p1, drop);
  const o4 = orient(q1, q2, p2, drop);
  if (((o1 > 0 && o2 < 0) || (o1 < 0 && o2 > 0)) && ((o3 > 0 && o4 < 0) || (o3 < 0 && o4 > 0))) {
    return true;
  }
  // touching: a point on the other segment
  const on = (a: Vec3LH, b: Vec3LH, c: Vec3LH, o: number): boolean =>
    Math.abs(o) <= EPS &&
    Math.min(a[u], b[u]) - EPS <= c[u] &&
    c[u] <= Math.max(a[u], b[u]) + EPS &&
    Math.min(a[v], b[v]) - EPS <= c[v] &&
    c[v] <= Math.max(a[v], b[v]) + EPS;
  return on(p1, p2, q1, o1) || on(p1, p2, q2, o2) || on(q1, q2, p1, o3) || on(q1, q2, p2, o4);
}

/** Moeller's triangle-triangle test (touching counts as crossing). */
export function trisIntersect(t1: Tri, t2: Tri): boolean {
  const n1 = cross3(sub(t1[1], t1[0]), sub(t1[2], t1[0]));
  const n2 = cross3(sub(t2[1], t2[0]), sub(t2[2], t2[0]));
  const l1 = Math.hypot(n1[0], n1[1], n1[2]);
  const l2 = Math.hypot(n2[0], n2[1], n2[2]);
  if (l1 < 1e-12 || l2 < 1e-12) return false; // a degenerate triangle crosses nothing
  const u1: Vec3LH = [n1[0] / l1, n1[1] / l1, n1[2] / l1];
  const u2: Vec3LH = [n2[0] / l2, n2[1] / l2, n2[2] / l2];
  const d2 = -dot(u2, t2[0]);
  const dv: [number, number, number] = [
    dot(u2, t1[0]) + d2,
    dot(u2, t1[1]) + d2,
    dot(u2, t1[2]) + d2,
  ];
  for (let i = 0; i < 3; i += 1) if (Math.abs(dv[i] ?? 0) < EPS) dv[i] = 0;
  if ((dv[0] > 0 && dv[1] > 0 && dv[2] > 0) || (dv[0] < 0 && dv[1] < 0 && dv[2] < 0)) return false;
  const d1 = -dot(u1, t1[0]);
  const du: [number, number, number] = [
    dot(u1, t2[0]) + d1,
    dot(u1, t2[1]) + d1,
    dot(u1, t2[2]) + d1,
  ];
  for (let i = 0; i < 3; i += 1) if (Math.abs(du[i] ?? 0) < EPS) du[i] = 0;
  if ((du[0] > 0 && du[1] > 0 && du[2] > 0) || (du[0] < 0 && du[1] < 0 && du[2] < 0)) return false;
  if (dv[0] === 0 && dv[1] === 0 && dv[2] === 0) {
    // coplanar: edge crossings, or one triangle inside the other
    const edges = (t: Tri): readonly (readonly [Vec3LH, Vec3LH])[] => [
      [t[0], t[1]],
      [t[1], t[2]],
      [t[2], t[0]],
    ];
    for (const [p1, p2] of edges(t1)) {
      for (const [q1, q2] of edges(t2)) {
        if (segmentsCross(p1, p2, q1, q2, u1)) return true;
      }
    }
    return pointInTri(t1[0], t2, u2) || pointInTri(t2[0], t1, u1);
  }
  // the direction of the intersection line and the interval of each triangle on it
  const dir = cross3(u1, u2);
  let axis = 0;
  if (Math.abs(dir[1]) > Math.abs(dir[0])) axis = 1;
  if (Math.abs(dir[2]) > Math.abs(dir[axis] ?? 0)) axis = 2;
  const interval = (tri: Tri, d: readonly [number, number, number]): [number, number] => {
    // the vertex alone on its side, then the two crossings of its edges with the plane
    const p = tri.map((v) => v[axis] ?? 0);
    const sgn = d.map((x) => (x > 0 ? 1 : x < 0 ? -1 : 0));
    let lone: number;
    if (sgn[1] === sgn[2] && sgn[0] !== sgn[1]) lone = 0;
    else if (sgn[0] === sgn[2] && sgn[1] !== sgn[0]) lone = 1;
    else if (sgn[0] === sgn[1] && sgn[2] !== sgn[0]) lone = 2;
    else {
      // a vertex on the plane: the interval is spanned by the vertices that are on it and the crossing of the opposite edge
      const pts: number[] = [];
      for (let i = 0; i < 3; i += 1) {
        const a = d[i] ?? 0;
        const b = d[(i + 1) % 3] ?? 0;
        if (a === 0) pts.push(p[i] ?? 0);
        if ((a > 0 && b < 0) || (a < 0 && b > 0)) {
          const k = a / (a - b);
          pts.push((p[i] ?? 0) + k * ((p[(i + 1) % 3] ?? 0) - (p[i] ?? 0)));
        }
      }
      return [Math.min(...pts), Math.max(...pts)];
    }
    const a = (lone + 1) % 3;
    const b = (lone + 2) % 3;
    const dl = d[lone] ?? 0;
    const ka = dl / (dl - (d[a] ?? 0));
    const kb = dl / (dl - (d[b] ?? 0));
    const pl = p[lone] ?? 0;
    const x = pl + ka * ((p[a] ?? 0) - pl);
    const y = pl + kb * ((p[b] ?? 0) - pl);
    return x < y ? [x, y] : [y, x];
  };
  const i1 = interval(t1, dv);
  const i2 = interval(t2, du);
  return i1[0] <= i2[1] && i2[0] <= i1[1];
}

/** The triangles of a mesh under `world`, in world space (LH). */
export function worldTriangles(mesh: DeployMesh, world: Mat16): Tri[] {
  const out: Tri[] = [];
  const p = mesh.positions;
  const ix = mesh.index;
  const vert = (i: number): Vec3LH =>
    transformCoord([p[i * 3] ?? 0, p[i * 3 + 1] ?? 0, p[i * 3 + 2] ?? 0], world);
  for (let t = 0; t + 2 < ix.length; t += 3) {
    out.push([vert(ix[t] ?? 0), vert(ix[t + 1] ?? 0), vert(ix[t + 2] ?? 0)]);
  }
  return out;
}

interface TriBox {
  readonly tri: Tri;
  readonly min: Vec3LH;
  readonly max: Vec3LH;
}

const boxOfTri = (tri: Tri): TriBox => ({
  tri,
  min: [
    Math.min(tri[0][0], tri[1][0], tri[2][0]),
    Math.min(tri[0][1], tri[1][1], tri[2][1]),
    Math.min(tri[0][2], tri[1][2], tri[2][2]),
  ],
  max: [
    Math.max(tri[0][0], tri[1][0], tri[2][0]),
    Math.max(tri[0][1], tri[1][1], tri[2][1]),
    Math.max(tri[0][2], tri[1][2], tri[2][2]),
  ],
});

const boxesOverlap = (a: TriBox, b: TriBox): boolean =>
  a.min[0] <= b.max[0] &&
  b.min[0] <= a.max[0] &&
  a.min[1] <= b.max[1] &&
  b.min[1] <= a.max[1] &&
  a.min[2] <= b.max[2] &&
  b.min[2] <= a.max[2];

/** Whether any triangle of `a` crosses any triangle of `b` (a bounding-box prefilter per triangle). */
export function meshesCross(a: readonly Tri[], b: readonly Tri[]): boolean {
  const bb = b.map(boxOfTri);
  for (const ta of a) {
    const box = boxOfTri(ta);
    for (const other of bb) {
      if (boxesOverlap(box, other) && trisIntersect(ta, other.tri)) return true;
    }
  }
  return false;
}

// ---- the whole test ------------------------------------------------------------------------------------------------------------------

/** What the collision of one pose needs besides the scene. */
export interface CollideInput {
  /** The scene (`null` while the models load: never red). */
  readonly scene: DeployScene | null;
  readonly pos: Vec3LH;
  readonly angle: number;
  /** The piece that does not count (a reinstall moves it). */
  readonly exceptObjId: number;
  /** The height attribute under the ghost (guild only; `undefined`/`null` = unknown, the check passes). */
  readonly heightAttr?: number | null | undefined;
  /** Test the height attribute first (the guild manager). */
  readonly guild: boolean;
}

/** The wall triangles in world space, computed once per scene object (the wall does not move). */
const wallCache = new WeakMap<DeployBody, Tri[]>();

function wallTriangles(wall: DeployBody): Tri[] {
  let t = wallCache.get(wall);
  if (!t) {
    t = worldTriangles(wall.mesh, wall.matrix);
    wallCache.set(wall, t);
  }
  return t;
}

/**
 * `CheckCollision()`: `true` = the ghost turns red. Order of the original: the height attribute (guild), then the wall (no wall object: never red), the ghost box against the wall box
 * (no overlap = outside of the room), the triangles, then every other ctrl box.
 */
export function collides(input: CollideInput): boolean {
  if (input.guild) {
    const a = input.heightAttr;
    if (a === HATTR_NOWALK || a === HATTR_NOFLY || a === HATTR_NOMOVE) return true;
  }
  const scene = input.scene;
  if (!scene?.wall) return false;
  const world = ghostMatrix(input.pos, input.angle);
  const ghostObb = obbOf(world, scene.ghost.bbox);
  const wallObb = obbOf(scene.wall.matrix, scene.wall.bbox);
  // the wall box wraps the whole room: not overlapping it means outside of the room
  if (!testObbIntersect(wallObb, ghostObb)) return true;
  const ghostTris = worldTriangles(scene.ghost.mesh, world);
  if (meshesCross(wallTriangles(scene.wall), ghostTris)) return true;
  for (const c of scene.ctrls) {
    if (c.id === input.exceptObjId) continue;
    if (testObbIntersect(obbOf(c.matrix, c.bbox), ghostObb)) return true;
  }
  return false;
}
