// The model geometry cache (Plan/41 section 11.3, 22.1 `MeshType`, WP-REND-03): `O3dMeta` + blob (the pack of
// `decode_o3d` with `Space::Three`: positions/normals mirrored, winding flipped, matrices mapped) become three.js
// geometry, one `BufferGeometry` per MATERIAL BLOCK (a draw range over the object's shared vertex/index
// attributes, so the GPU buffers exist once per object), grouped per LOD group and object.
//
// Attributes: `position`, `normal`, `uv` (as the shaders declare them); skinned objects add `aJoints` (u16 x 2,
// integer attribute: global `.chr` bone indices) and `aWeights` (f32 x 2) for WP-REND-04's palette skinning (the
// bind-pose materials of this WP ignore them). The typed arrays are views over the decoder blob, no copies.

import { Box3, BufferAttribute, BufferGeometry, IntType, Matrix4, Sphere, Vector3 } from 'three';
import type { AttrMeta, O3dMeta, ObjectMeta, ParentRef, Submesh } from '../../gen/formats';
import { view } from '../../wasm/decoders';
import type { GpuAccounting } from '../gl/uploadQueue';

/** Thrown for a pack whose sections do not fit its blob (corrupt meta). */
export class ModelBuildError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ModelBuildError';
  }
}

/** One draw call of the original: a material block (`MATERIAL_BLOCK`). */
export interface BlockGeometry {
  /** Shares the object's attributes; `drawRange` = the block's index range. */
  readonly geometry: BufferGeometry;
  readonly submesh: Submesh;
  readonly blockIndex: number;
}

/** One GMOBJECT of a LOD group. */
export interface ObjectGeometry {
  readonly index: number;
  readonly kind: ObjectMeta['kind'];
  /** Night-only light object (`ObjectMeta.light`). */
  readonly light: boolean;
  readonly parent: ParentRef;
  /** `localTM`, mapped (three.js column-major elements). */
  readonly localTM: Matrix4;
  /** `det3(localTM) < 0` in LH space (29 corpus objects). */
  readonly mirrored: boolean;
  readonly bbox: Box3;
  readonly vertexCount: number;
  readonly triangleCount: number;
  readonly blocks: readonly BlockGeometry[];
  /** Node animation, `qx qy qz qw px py pz` per frame (mapped), `null` when the object is not node-animated. */
  readonly anim: Float32Array | null;
  /** CPU copies (views) for precise picking: object-local positions and the triangle index. */
  readonly pick: { readonly positions: Float32Array; readonly index: Uint16Array | Uint32Array };
}

/** The geometry of one `.o3d`. */
export interface ModelGeometry {
  readonly key: string;
  /** The pack meta (sections reference the blob). */
  readonly meta: O3dMeta;
  /** `lods[group][object]`. */
  readonly lods: readonly (readonly ObjectGeometry[])[];
  readonly bbox: Box3;
  readonly perSlerp: number;
  readonly maxFrame: number;
  /** Frame attributes of the built-in animation (hit/sound/quake, 0xB0...). */
  readonly attrs: readonly AttrMeta[];
  /** Sword-trail / enchant line points (0, 2 or 4), model space, mapped. */
  readonly force: readonly (readonly [number, number, number])[];
  readonly events: readonly (readonly [number, number, number])[];
  /** Embedded `FSK1`/`FMO1` sections (LH, unmapped) for WP-REND-04 and the sim. */
  readonly embedded: { readonly skeleton: Uint8Array; readonly motion: Uint8Array | null } | null;
  /** The node clip (`FMO1` flag bit 2) of node-animated files. */
  readonly nodeClip: Uint8Array | null;
  /** Estimated GPU bytes of the buffers (positions, normals, uvs, skin attributes, indices). */
  readonly gpuBytes: number;
  /** Number of LOD groups. */
  readonly lodCount: number;
  /** The texture names of every block of `lod` (lower-case, no directory), de-duplicated. */
  textureNames(lod?: number): string[];
  dispose(): void;
}

export interface BuildOptions {
  /** Cache key / debugging name (the VFS path). */
  readonly key?: string;
}

const mat = (a: ArrayLike<number>): Matrix4 => new Matrix4().fromArray(Array.from(a));

/**
 * The box of a header `[minX, minY, minZ, maxX, maxY, maxZ]`, normalised per axis. 82 of the 4196 `.o3d` header boxes store
 * min > max on an axis (`Obj_MaMaPrAmmo.o3d` x 3.14..-3.0): three.js treats such a `Box3` as empty (centre and size 0), which
 * gave the culling sphere of those props radius 0. The original builds the 8 OBB corners from `m_vMin`/`m_vMax`, which does not
 * depend on the order (Obj.cpp:426-455), so the box works there (BFIX1 B3, #015/#018).
 */
function boxOf(b: readonly number[]): Box3 {
  const [x0, y0, z0, x1, y1, z1] = [
    b[0] ?? 0,
    b[1] ?? 0,
    b[2] ?? 0,
    b[3] ?? 0,
    b[4] ?? 0,
    b[5] ?? 0,
  ];
  return new Box3(
    new Vector3(Math.min(x0, x1), Math.min(y0, y1), Math.min(z0, z1)),
    new Vector3(Math.max(x0, x1), Math.max(y0, y1), Math.max(z0, z1)),
  );
}

function buildObject(
  o: ObjectMeta,
  index: number,
  blob: Uint8Array,
): { geo: ObjectGeometry; bytes: number } {
  const positions = view(blob, { ...o.position, ty: 'f32' });
  const normals = view(blob, { ...o.normal, ty: 'f32' });
  const uvs = view(blob, { ...o.uv, ty: 'f32' });
  const indexArray: Uint16Array | Uint32Array =
    o.index.ty === 'u32'
      ? view(blob, { ...o.index, ty: 'u32' })
      : view(blob, { ...o.index, ty: 'u16' });
  if (positions.length !== o.vertex_count * 3) {
    throw new ModelBuildError(
      `object ${String(index)}: position section does not match vertex_count`,
    );
  }

  const position = new BufferAttribute(positions, 3);
  const normal = new BufferAttribute(normals, 3);
  const uv = new BufferAttribute(uvs, 2);
  const idx = new BufferAttribute(indexArray, 1);
  let joints: BufferAttribute | null = null;
  let weights: BufferAttribute | null = null;
  let bytes = positions.byteLength + normals.byteLength + uvs.byteLength + indexArray.byteLength;
  if (o.kind === 'skin' && o.joints && o.weights) {
    const j = view(blob, { ...o.joints, ty: 'u16' });
    const w = view(blob, { ...o.weights, ty: 'f32' });
    joints = new BufferAttribute(j, 2);
    joints.gpuType = IntType;
    weights = new BufferAttribute(w, 2);
    bytes += j.byteLength + w.byteLength;
  }

  const bbox = boxOf(o.bbox);
  const sphere = bbox.getBoundingSphere(new Sphere());
  const blocks: BlockGeometry[] = [];
  o.submeshes.forEach((sub, blockIndex) => {
    if (sub.start + sub.count > indexArray.length) {
      throw new ModelBuildError(
        `object ${String(index)} block ${String(blockIndex)} exceeds the index section`,
      );
    }
    const g = new BufferGeometry();
    g.setAttribute('position', position);
    g.setAttribute('normal', normal);
    g.setAttribute('uv', uv);
    if (joints && weights) {
      g.setAttribute('aJoints', joints);
      g.setAttribute('aWeights', weights);
    }
    g.setIndex(idx);
    g.setDrawRange(sub.start, sub.count);
    g.boundingBox = bbox.clone();
    g.boundingSphere = sphere.clone();
    blocks.push({ geometry: g, submesh: sub, blockIndex });
  });

  const anim = o.anim ? view(blob, { ...o.anim, ty: 'f32' }) : null;
  return {
    geo: {
      index,
      kind: o.kind,
      light: o.light,
      parent: o.parent,
      localTM: mat(o.local_tm),
      mirrored: o.mirrored,
      bbox,
      vertexCount: o.vertex_count,
      triangleCount: Math.floor(indexArray.length / 3),
      blocks,
      anim,
      pick: { positions, index: indexArray },
    },
    bytes,
  };
}

/** Builds the geometry of a decoded mesh pack. Throws `ModelBuildError` for sections that do not fit. */
export function buildModelGeometry(
  meta: O3dMeta,
  blob: Uint8Array,
  options: BuildOptions = {},
): ModelGeometry {
  let bytes = 0;
  const lods: ObjectGeometry[][] = [];
  try {
    for (const lod of meta.lods) {
      const objs: ObjectGeometry[] = [];
      lod.objects.forEach((o, i) => {
        const b = buildObject(o, i, blob);
        bytes += b.bytes;
        objs.push(b.geo);
      });
      lods.push(objs);
    }
  } catch (e) {
    for (const g of lods) for (const o of g) for (const b of o.blocks) b.geometry.dispose();
    if (e instanceof RangeError) throw new ModelBuildError(`o3d pack: ${e.message}`);
    throw e;
  }
  const embedded = meta.embedded
    ? {
        skeleton: view(blob, { ...meta.embedded.skeleton, ty: 'u8' }),
        motion: meta.embedded.motion ? view(blob, { ...meta.embedded.motion, ty: 'u8' }) : null,
      }
    : null;
  const nodeClip = meta.node_clip ? view(blob, { ...meta.node_clip, ty: 'u8' }) : null;
  const bbox = boxOf(meta.bbox);
  return {
    key: options.key ?? '',
    meta,
    lods,
    bbox,
    perSlerp: meta.per_slerp,
    maxFrame: meta.max_frame,
    attrs: meta.attrs,
    force: meta.force,
    events: meta.events,
    embedded,
    nodeClip,
    gpuBytes: bytes,
    lodCount: lods.length,
    textureNames(lod = 0) {
      const names = new Set<string>();
      for (const o of lods[Math.min(lod, lods.length - 1)] ?? []) {
        for (const b of o.blocks) if (b.submesh.texture) names.add(b.submesh.texture);
      }
      return [...names];
    },
    dispose() {
      for (const g of lods) for (const o of g) for (const b of o.blocks) b.geometry.dispose();
    },
  };
}

/** Ref-counted cache of model geometry by key (the VFS path). */
export class ModelCache {
  private readonly entries = new Map<
    string,
    { model: ModelGeometry; refs: number; lastUsed: number }
  >();

  /** `accounting` receives the estimated GPU bytes of every resident model (key `model:<key>`). */
  constructor(private readonly accounting?: GpuAccounting) {}

  /** Registers a decoded model; an existing entry of the key is kept and returned (its refcount increases). */
  add(key: string, meta: O3dMeta, blob: Uint8Array): ModelGeometry {
    const existing = this.entries.get(key);
    if (existing) {
      existing.refs++;
      return existing.model;
    }
    const model = buildModelGeometry(meta, blob, { key });
    this.entries.set(key, { model, refs: 1, lastUsed: performance.now() });
    this.accounting?.set(`model:${key}`, model.gpuBytes);
    return model;
  }

  /** Acquires a resident model (refcount + 1), `null` when it is not resident. */
  get(key: string): ModelGeometry | null {
    const e = this.entries.get(key);
    if (!e) return null;
    e.refs++;
    e.lastUsed = performance.now();
    return e.model;
  }

  has(key: string): boolean {
    return this.entries.has(key);
  }

  release(key: string): void {
    const e = this.entries.get(key);
    if (!e) return;
    e.refs = Math.max(0, e.refs - 1);
    e.lastUsed = performance.now();
  }

  /** Disposes zero-ref models idle for more than `maxIdleMs` (or all of them with 0); returns how many. */
  trim(maxIdleMs: number, now = performance.now()): number {
    let n = 0;
    for (const [key, e] of [...this.entries]) {
      if (e.refs === 0 && now - e.lastUsed >= maxIdleMs) {
        e.model.dispose();
        this.entries.delete(key);
        this.accounting?.remove(`model:${key}`);
        n++;
      }
    }
    return n;
  }

  get size(): number {
    return this.entries.size;
  }

  /** Estimated GPU bytes of every resident model. */
  get gpuBytes(): number {
    let n = 0;
    for (const e of this.entries.values()) n += e.model.gpuBytes;
    return n;
  }

  /**
   * The WebGL context is back: three.js uploads the buffers again on the next draw (the arrays are retained), so
   * the accounting is simply re-filled.
   */
  onContextRestored(): void {
    for (const [key, e] of this.entries) this.accounting?.set(`model:${key}`, e.model.gpuBytes);
  }

  dispose(): void {
    for (const [key, e] of this.entries) {
      e.model.dispose();
      this.accounting?.remove(`model:${key}`);
    }
    this.entries.clear();
  }
}
