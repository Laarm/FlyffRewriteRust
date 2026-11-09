// Picking support of the renderer (Plan/41 section 20, WP-REND-15; algorithm and filters: doc 43 section 5): what doc 43 needs
// to decide WHICH drawn object is under the pointer.
//
//   visibleObjects()      the proxies of the objects drawn in the last frame (movers now; ground items and ctrls register their
//                         own sources with WP-REND-22/23): world matrix `Scale * Rotation * Translate(logical pos)` (a pet: its
//                         smoothed screen position) and the model box in that matrix' space; the flags the candidate filters read
//   raycastPrecise(id)    the triangle test of the current pose: movers through the primary bone of every vertex (palette matrix
//                         of the first joint), rigid objects through their mesh. Boss ranks and ctrls are the clients of it
//   projectBounds(id)     the screen rectangle (CSS px) of the 8 projected corners of the box (`GetBoundRect`, WndWorld.cpp:2670)
//
// The sources are registered by kind; every function answers `null` for an id no source owns (not drawn in the last frame).

import { Matrix4 } from 'three';
import type { RenderCamera } from '../camera';
import type { MoverBatcher, MoverEntry } from '../movers/MoverBatcher';
import type { MoverVisual } from '../movers/MoverVisual';
import { MOVER_FLAGS, newPoseFrame, type PoseSource } from '../movers/poseSource';
import { worldMatrixThree, type Placement } from '../objects/placement';
import { XE } from '../gl/materials/key';
import { rows3ToMatrix4 } from '../space';
import type { PickProxy, Ray3, ScreenRect } from '../types';
import { FLOATS_PER_MATRIX } from '../movers/PaletteTexture';
import { rayVsMesh, type PickMesh } from './meshRay';

/** `GCWAR_NOT_CLICK_MODE`. */
export const MODE_GCWAR_NOT_CLICK = 0x0008_0000;

/** What a proxy source answers (movers here; items and ctrls register their own). */
export interface PickSource {
  /** Appends the proxies of the objects drawn in the last frame. */
  collect(out: PickProxy[]): void;
  /** The world matrix and model box of a drawn object, `undefined` when this source does not own `id`. */
  boundsOf(
    id: number,
  ): { matrix: Matrix4; aabb: readonly [readonly number[], readonly number[]] } | undefined;
  /** The precise test: the distance, `null` for a miss, `undefined` when this source does not own `id`. */
  raycast(id: number, ray: Ray3): number | null | undefined;
}

export interface MoverPickDeps {
  readonly batcher: MoverBatcher;
  readonly pose: PoseSource;
}

/** The first local object id (`game/pets/eggPet.ts`): the local system pets are unknown to the server, so they are never picked (WP-CLIENT-31). */
const LOCAL_ID_BASE = 0xff00_0000;
const FALLBACK_HALF = 0.4;
const FALLBACK_HEIGHT = 1.8;

/** The mover proxies: the batcher's drawn movers. */
export class MoverPickSource implements PickSource {
  private readonly scratch = newPoseFrame();
  private readonly cache = new Map<number, PickProxy>();

  constructor(private readonly d: MoverPickDeps) {}

  /** `Scale * Rotation * Translate(pos)` of a mover in three.js space (the logical placement, no heel). */
  static matrixOf(e: MoverEntry, scale: number, out: Matrix4 = new Matrix4()): Matrix4 {
    const pos = e.pet ? e.drawPos : e.pos;
    const p: Placement = {
      pos: [pos[0], pos[1], pos[2]],
      angle: e.angle,
      angX: 0,
      angZ: 0,
      scale: [scale, scale, scale],
    };
    return worldMatrixThree(p, out);
  }

  private aabbOf(
    e: MoverEntry,
    visual: MoverVisual,
  ): readonly [readonly number[], readonly number[]] {
    const b = visual.bounds;
    if (!b.isEmpty()) return [b.min.toArray(), b.max.toArray()];
    const r = Math.max(e.radius, FALLBACK_HALF);
    return [
      [-r, 0, -r],
      [r, Math.max(FALLBACK_HEIGHT, 2 * r), r],
    ];
  }

  collect(out: PickProxy[]): void {
    this.d.batcher.forEachDrawn((e, visual) => {
      if (e.id >= LOCAL_ID_BASE) return;
      const scale = e.rec.visual?.scale ?? 1;
      const aabb = this.aabbOf(e, visual);
      const mode = e.rec.visual?.mode ?? 0;
      const matrix = this.cache.get(e.id)?.worldMatrix ?? new Matrix4();
      MoverPickSource.matrixOf(e, scale > 0 ? scale : 1, matrix);
      const proxy: PickProxy = {
        id: e.id,
        kind: 'mover',
        worldMatrix: matrix,
        localAabb: [
          [aabb[0][0] ?? 0, aabb[0][1] ?? 0, aabb[0][2] ?? 0],
          [aabb[1][0] ?? 0, aabb[1][1] ?? 0, aabb[1][2] ?? 0],
        ],
        dead: (e.flags & MOVER_FLAGS.DEAD) !== 0,
        isPlayer: e.rec.visual?.kind === 'player',
        rank: e.rank,
        ai: e.ai,
        ownPet: e.pet && (e.rec as { ownPet?: boolean }).ownPet === true,
        transparent: !e.decorated,
        gcwarNotClick: (mode & MODE_GCWAR_NOT_CLICK) !== 0,
      };
      this.cache.set(e.id, proxy);
      out.push(proxy);
    });
    // forget movers that were not drawn this frame
    if (this.cache.size > out.length * 2 + 16) {
      const live = new Set(out.map((p) => p.id));
      for (const id of this.cache.keys()) if (!live.has(id)) this.cache.delete(id);
    }
  }

  boundsOf(id: number) {
    const e = this.d.batcher.entry(id);
    if (!e?.drawn || !e.visual) return undefined;
    const scale = e.rec.visual?.scale ?? 1;
    return {
      matrix: MoverPickSource.matrixOf(e, scale > 0 ? scale : 1),
      aabb: this.aabbOf(e, e.visual),
    };
  }

  raycast(id: number, ray: Ray3): number | null | undefined {
    const e = this.d.batcher.entry(id);
    if (!e?.drawn || !e.visual) return undefined;
    const fr = this.scratch;
    if (!this.d.pose.read(id, fr) || !fr.palette) return null;
    const world = e.visual.root.matrix; // the pose world matrix of the last frame (scale, heel and shake included)
    const palette = fr.palette;
    const base = fr.paletteOffset;
    const boneCount = fr.boneCount;
    const m = new Matrix4();
    const bone = new Matrix4();
    let best: number | null = null;
    for (const model of e.visual.pickModels()) {
      for (const obj of model.lods[0] ?? []) {
        if (obj.kind !== 'skin' || obj.light) continue;
        const first = obj.blocks[0]?.geometry.getAttribute('aJoints');
        if (!first) continue;
        const mesh: PickMesh = {
          positions: obj.pick.positions,
          index: obj.pick.index,
          joints: first.array,
          blocks: obj.blocks.map((b) => ({
            start: b.submesh.start,
            count: b.submesh.count,
            twoSided: (b.submesh.effect & XE.TWO_SIDE) !== 0,
          })),
        };
        const t = rayVsMesh(ray, mesh, (joint) => {
          if (joint >= boneCount) return null;
          rows3ToMatrix4(palette, base + joint * FLOATS_PER_MATRIX, bone);
          return m.multiplyMatrices(world, bone).clone();
        });
        if (t !== null && (best === null || t < best)) best = t;
      }
    }
    return best;
  }
}

/** The aggregate the renderer exposes (`visibleObjects`, `raycastPrecise`, `projectBounds`). */
export class PickSupport {
  private readonly sources: PickSource[] = [];
  private proxies: PickProxy[] = [];
  private builtFor = -1;

  constructor(
    private readonly camera: RenderCamera,
    /** The canvas CSS size (the viewport of the projection). */
    private readonly viewport: () => { readonly width: number; readonly height: number },
    /** A number that changes with every drawn frame (the batcher's frame counter). */
    private readonly frameId: () => number,
  ) {}

  /** Adds a source (ground items, ctrls): its proxies join `visibleObjects()`. */
  register(source: PickSource): () => void {
    this.sources.push(source);
    this.builtFor = -1;
    return () => {
      const i = this.sources.indexOf(source);
      if (i >= 0) this.sources.splice(i, 1);
      this.builtFor = -1;
    };
  }

  /** The proxies of the objects drawn in the last frame (rebuilt once per drawn frame). */
  visibleObjects(): readonly PickProxy[] {
    const id = this.frameId();
    if (id !== this.builtFor) {
      const out: PickProxy[] = [];
      for (const s of this.sources) s.collect(out);
      this.proxies = out;
      this.builtFor = id;
    }
    return this.proxies;
  }

  /** The distance of the nearest hit along the ray (three.js space, unit direction), `null` for a miss or an unknown id. */
  raycastPrecise(id: number, ray: Ray3): number | null {
    for (const s of this.sources) {
      const r = s.raycast(id, ray);
      if (r !== undefined) return r;
    }
    return null;
  }

  /** The screen rectangle (CSS px, y down) of the projected box corners, `null` when the object is not drawn or behind the camera. */
  projectBounds(id: number): ScreenRect | null {
    for (const s of this.sources) {
      const b = s.boundsOf(id);
      if (b) return projectBox(this.camera, this.viewport(), b.matrix, b.aabb);
    }
    return null;
  }
}

/** The bounding rectangle of the 8 corners of `aabb` under `matrix` projected through the camera (D3DXVec3Project). */
export function projectBox(
  camera: RenderCamera,
  viewport: { readonly width: number; readonly height: number },
  matrix: Matrix4,
  aabb: readonly [readonly number[], readonly number[]],
): ScreenRect | null {
  const vp = camera.viewProjection.elements;
  const w = matrix.elements;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  let any = false;
  for (let i = 0; i < 8; i++) {
    const x = (i & 1) === 0 ? (aabb[0][0] ?? 0) : (aabb[1][0] ?? 0);
    const y = (i & 2) === 0 ? (aabb[0][1] ?? 0) : (aabb[1][1] ?? 0);
    const z = (i & 4) === 0 ? (aabb[0][2] ?? 0) : (aabb[1][2] ?? 0);
    // world
    const wx = w[0] * x + w[4] * y + w[8] * z + w[12];
    const wy = w[1] * x + w[5] * y + w[9] * z + w[13];
    const wz = w[2] * x + w[6] * y + w[10] * z + w[14];
    // clip
    const cw = vp[3] * wx + vp[7] * wy + vp[11] * wz + vp[15];
    if (cw <= 0) continue; // behind the camera
    const cx = vp[0] * wx + vp[4] * wy + vp[8] * wz + vp[12];
    const cy = vp[1] * wx + vp[5] * wy + vp[9] * wz + vp[13];
    const px = ((cx / cw + 1) * viewport.width) / 2;
    const py = ((1 - cy / cw) * viewport.height) / 2;
    any = true;
    if (px < minX) minX = px;
    if (px > maxX) maxX = px;
    if (py < minY) minY = py;
    if (py > maxY) maxY = py;
  }
  if (!any) return null;
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}
