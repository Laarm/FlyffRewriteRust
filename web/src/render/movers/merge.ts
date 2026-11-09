// Part merging per mover (Plan/41 section 12.7 contingency, WP-REND-05 "part merging per mover for crowds"): the skinned,
// non-hair parts of ONE mover (head, body parts, armour, cloak, ... or the blocks of a whole monster model) are
// concatenated into ONE geometry per material state, so a player costs 1-3 draw calls instead of one per part and block.
//
//  * every material block contributes the vertices its index range references (copied, so a vertex shared by two blocks
//    with different textures is duplicated: the texture is a per-vertex slot),
//  * the texture of a block becomes a SLOT: an index into the (at most `MERGE_SLOTS` = 8) distinct texture names of the
//    merged set; the fragment shader samples `uMap<slot>` (see `modelShader.ts`, `slots`),
//  * blocks are grouped by their material STATE `(effect, amount)` (the inputs of `deriveMaterial`): one draw per group.
//    The grouping is independent of the per-mover model blend, so the geometry is shared by every mover with the same
//    parts (`MergedGeometryCache`), only the materials are per mover.
//
// The merge is exact: positions, normals, UVs, joints and weights are copied, the index order inside a block is kept,
// so the picture equals the one of the separate parts (verified by the Playwright image comparison).

import { BufferAttribute, BufferGeometry, Box3, IntType, Sphere, Vector3 } from 'three';
import type { ModelGeometry, ObjectGeometry } from '../objects/ModelCache';

/** Texture slots of a merged draw (the `uMap0 .. uMap7` samplers of the fragment shader). */
export const MERGE_SLOTS = 8;

/** One draw of the merged geometry: the blocks with the same material state. */
export interface MergedGroup {
  /** `Submesh.effect` (`XE_*`). */
  readonly effect: number;
  /** `Submesh.amount`. */
  readonly amount: number;
  /** The index range of the group. */
  readonly start: number;
  readonly count: number;
}

/** What `mergeSkinnedParts` builds. */
export interface MergedGeometry {
  readonly lod: number;
  /** The shared vertex attributes (`position`, `normal`, `uv`, `aJoints`, `aWeights`, `aSlot`). */
  readonly attributes: {
    readonly position: BufferAttribute;
    readonly normal: BufferAttribute;
    readonly uv: BufferAttribute;
    readonly aJoints: BufferAttribute;
    readonly aWeights: BufferAttribute;
    readonly aSlot: BufferAttribute;
  };
  readonly index: BufferAttribute;
  readonly groups: readonly MergedGroup[];
  /** One geometry per group (shared attributes, `drawRange` = the group). */
  readonly geometries: readonly BufferGeometry[];
  /** Base texture name per slot (lower-case, no directory; `''` = untextured). */
  readonly slotTextures: readonly string[];
  /** `slotOf[partIndex][objectIndex][blockIndex]` = the slot of that block. */
  readonly slotOf: readonly (readonly (readonly number[])[])[];
  readonly vertexCount: number;
  readonly triangleCount: number;
  readonly bbox: Box3;
  /** Estimated GPU bytes. */
  readonly gpuBytes: number;
  dispose(): void;
}

/** A part that can be merged: every object of the LOD group is skinned. */
export function isMergeable(model: ModelGeometry, lod: number): boolean {
  const objs = model.lods[Math.min(lod, model.lodCount - 1)];
  if (!objs || objs.length === 0) return false;
  return objs.every((o) => o.kind === 'skin' && !o.light && o.blocks.length > 0);
}

const _v = new Vector3();

const objectsOf = (m: ModelGeometry, lod: number): readonly ObjectGeometry[] =>
  m.lods[Math.min(lod, m.lodCount - 1)] ?? [];

/** The distinct base texture names of `parts` at `lod`, in order of first use (slot assignment). */
export function slotTextureNames(parts: readonly ModelGeometry[], lod: number): string[] {
  const names: string[] = [];
  for (const p of parts) {
    for (const o of objectsOf(p, lod)) {
      for (const b of o.blocks) {
        const n = b.submesh.texture ?? '';
        if (!names.includes(n)) names.push(n);
      }
    }
  }
  return names;
}

/**
 * The longest prefix of `parts` (in the given order) whose blocks use at most `MERGE_SLOTS` distinct textures at
 * every LOD and that is mergeable; parts after the prefix stay separate instances.
 */
export function mergeablePrefix(parts: readonly ModelGeometry[]): number {
  let n = 0;
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (!p) break;
    const next = parts.slice(0, i + 1);
    let ok = true;
    for (let lod = 0; lod < Math.max(...next.map((m) => m.lodCount)); lod++) {
      if (
        !next.every((m) => isMergeable(m, lod)) ||
        slotTextureNames(next, lod).length > MERGE_SLOTS
      ) {
        ok = false;
        break;
      }
    }
    if (!ok) break;
    n = i + 1;
  }
  return n;
}

/**
 * Merges the blocks of `parts` at `lod` into one geometry per material state. `parts` must satisfy `isMergeable` at
 * `lod` and use at most `MERGE_SLOTS` texture names (`mergeablePrefix`); otherwise `null`.
 */
export function mergeSkinnedParts(
  parts: readonly ModelGeometry[],
  lod: number,
): MergedGeometry | null {
  if (parts.length === 0 || !parts.every((p) => isMergeable(p, lod))) return null;
  const slotTextures = slotTextureNames(parts, lod);
  if (slotTextures.length > MERGE_SLOTS) return null;

  // blocks in order, tagged with their material state and slot
  interface Blk {
    readonly obj: ObjectGeometry;
    readonly start: number;
    readonly count: number;
    readonly slot: number;
    readonly state: string;
    readonly effect: number;
    readonly amount: number;
  }
  const blocks: Blk[] = [];
  const slotOf: number[][][] = [];
  parts.forEach((p, pi) => {
    const perObject: number[][] = [];
    for (const o of objectsOf(p, lod)) {
      const perBlock: number[] = [];
      for (const b of o.blocks) {
        const slot = slotTextures.indexOf(b.submesh.texture ?? '');
        perBlock.push(slot);
        blocks.push({
          obj: o,
          start: b.submesh.start,
          count: b.submesh.count,
          slot,
          state: `${String(b.submesh.effect)}/${String(b.submesh.amount)}`,
          effect: b.submesh.effect,
          amount: b.submesh.amount,
        });
      }
      perObject.push(perBlock);
    }
    slotOf[pi] = perObject;
  });
  // stable order of the groups: first appearance of their state
  const states: string[] = [];
  for (const b of blocks) if (!states.includes(b.state)) states.push(b.state);
  const ordered = states.flatMap((s) => blocks.filter((b) => b.state === s));

  // vertex budget: the vertices each block references (unique per block)
  const remaps = ordered.map((b) => {
    const idx = b.obj.pick.index;
    const used = new Map<number, number>();
    for (let i = b.start; i < b.start + b.count; i++) {
      const v = idx[i];
      if (v !== undefined && !used.has(v)) used.set(v, used.size);
    }
    return used;
  });
  const vertexCount = remaps.reduce((n, m) => n + m.size, 0);
  const indexCount = ordered.reduce((n, b) => n + b.count - (b.count % 3), 0);
  const position = new Float32Array(vertexCount * 3);
  const normal = new Float32Array(vertexCount * 3);
  const uv = new Float32Array(vertexCount * 2);
  const joints = new Uint16Array(vertexCount * 2);
  const weights = new Float32Array(vertexCount * 2);
  const slots = new Float32Array(vertexCount);
  const index = new Uint32Array(indexCount);
  const box = new Box3().makeEmpty();
  const groups: MergedGroup[] = [];
  let vBase = 0;
  let iAt = 0;
  ordered.forEach((b, bi) => {
    const remap = remaps[bi];
    if (!remap) return;
    const geo = b.obj.blocks[0]?.geometry;
    const attrs = geo?.attributes;
    const sp = attrs?.['position'];
    const sn = attrs?.['normal'];
    const su = attrs?.['uv'];
    const sj = attrs?.['aJoints'];
    const sw = attrs?.['aWeights'];
    if (!sp || !sn || !su || !sj || !sw) return;
    const pa = sp.array as ArrayLike<number>;
    const na = sn.array as ArrayLike<number>;
    const ua = su.array as ArrayLike<number>;
    const ja = sj.array as ArrayLike<number>;
    const wa = sw.array as ArrayLike<number>;
    for (const [src, dst] of remap) {
      const d = vBase + dst;
      for (let k = 0; k < 3; k++) {
        position[d * 3 + k] = pa[src * 3 + k] ?? 0;
        normal[d * 3 + k] = na[src * 3 + k] ?? 0;
      }
      uv[d * 2] = ua[src * 2] ?? 0;
      uv[d * 2 + 1] = ua[src * 2 + 1] ?? 0;
      joints[d * 2] = ja[src * 2] ?? 0;
      joints[d * 2 + 1] = ja[src * 2 + 1] ?? 0;
      weights[d * 2] = wa[src * 2] ?? 0;
      weights[d * 2 + 1] = wa[src * 2 + 1] ?? 0;
      slots[d] = b.slot;
      box.expandByPoint(
        _v.set(position[d * 3] ?? 0, position[d * 3 + 1] ?? 0, position[d * 3 + 2] ?? 0),
      );
    }
    const idx = b.obj.pick.index;
    const first = iAt;
    for (let i = b.start; i + 2 < b.start + b.count; i += 3) {
      for (let k = 0; k < 3; k++) {
        const v = idx[i + k] ?? 0;
        index[iAt++] = vBase + (remap.get(v) ?? 0);
      }
    }
    const last = groups[groups.length - 1];
    if (last?.effect === b.effect && last.amount === b.amount) {
      groups[groups.length - 1] = { ...last, count: last.count + (iAt - first) };
    } else {
      groups.push({ effect: b.effect, amount: b.amount, start: first, count: iAt - first });
    }
    vBase += remap.size;
  });

  const attributes = {
    position: new BufferAttribute(position, 3),
    normal: new BufferAttribute(normal, 3),
    uv: new BufferAttribute(uv, 2),
    aJoints: new BufferAttribute(joints, 2),
    aWeights: new BufferAttribute(weights, 2),
    aSlot: new BufferAttribute(slots, 1),
  };
  attributes.aJoints.gpuType = IntType;
  const indexAttr = new BufferAttribute(index, 1);
  const sphere = box.getBoundingSphere(new Sphere());
  const geometries = groups.map((g) => {
    const geo = new BufferGeometry();
    geo.setAttribute('position', attributes.position);
    geo.setAttribute('normal', attributes.normal);
    geo.setAttribute('uv', attributes.uv);
    geo.setAttribute('aJoints', attributes.aJoints);
    geo.setAttribute('aWeights', attributes.aWeights);
    geo.setAttribute('aSlot', attributes.aSlot);
    geo.setIndex(indexAttr);
    geo.setDrawRange(g.start, g.count);
    geo.boundingBox = box.clone();
    geo.boundingSphere = sphere.clone();
    return geo;
  });
  const bytes =
    position.byteLength +
    normal.byteLength +
    uv.byteLength +
    joints.byteLength +
    weights.byteLength +
    slots.byteLength +
    index.byteLength;
  return {
    lod,
    attributes,
    index: indexAttr,
    groups,
    geometries,
    slotTextures,
    slotOf,
    vertexCount,
    triangleCount: indexCount / 3,
    bbox: box,
    gpuBytes: bytes,
    dispose() {
      for (const g of geometries) g.dispose();
    },
  };
}

/** Shared merged geometries by `lod|file+file+...` (movers with the same parts share one). Reference counted. */
export class MergedGeometryCache {
  private readonly entries = new Map<string, { geo: MergedGeometry | null; refs: number }>();

  /** The merged geometry of `files` (resolved to `parts`) at `lod`; `null` when it cannot be merged. */
  acquire(
    files: readonly string[],
    parts: readonly ModelGeometry[],
    lod: number,
  ): MergedGeometry | null {
    const key = `${String(lod)}|${files.join('+')}`;
    let e = this.entries.get(key);
    if (!e) {
      e = { geo: mergeSkinnedParts(parts, lod), refs: 0 };
      this.entries.set(key, e);
    }
    e.refs++;
    return e.geo;
  }

  release(files: readonly string[], lod: number): void {
    const e = this.entries.get(`${String(lod)}|${files.join('+')}`);
    if (e) e.refs = Math.max(0, e.refs - 1);
  }

  /** Disposes unreferenced entries; returns how many. */
  trim(): number {
    let n = 0;
    for (const [k, e] of [...this.entries]) {
      if (e.refs === 0) {
        e.geo?.dispose();
        this.entries.delete(k);
        n++;
      }
    }
    return n;
  }

  get size(): number {
    return this.entries.size;
  }

  get gpuBytes(): number {
    let n = 0;
    for (const e of this.entries.values()) n += e.geo?.gpuBytes ?? 0;
    return n;
  }

  dispose(): void {
    for (const e of this.entries.values()) e.geo?.dispose();
    this.entries.clear();
  }
}
