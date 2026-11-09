// Instanced geometry helpers (BFIX1 render-statics, bugs #015/#018).
//
// three.js 0.186 caches the instance limit of an `InstancedBufferGeometry` at its FIRST draw: `WebGLBindingStates.js:373-377`
// and `:415-419` set `geometry._maxInstanceCount = meshPerAttribute * count` of the first instanced attribute, but only while the
// field is still `undefined`; `WebGLRenderer.js:1343-1346` then draws `Math.min(geometry.instanceCount, _maxInstanceCount)`, and
// `WebGLGeometries.js:39-41` deletes the field only in the geometry's `dispose` handler. A geometry that swaps in bigger
// per-instance attributes after its first draw (the old `grow()` of the static groups, the mover batches and the blob shadows)
// therefore keeps drawing the instance count it had at that draw: the instances beyond it are silently missing, and which ones
// follows the order of the visible list, i.e. the camera angle.
//
// The rule of this module: every instanced growth goes through `growInstanced` (it builds a NEW geometry, which three.js has
// never drawn, so its first draw caches the new capacity), and every instanced geometry is released with
// `releaseInstancedGeometry` (it detaches the attributes and the index that belong to a source geometry before `dispose()`, so
// three.js frees the instance buffers, the VAOs and the cached limit but not the shared model buffers).
//
// No private field of three.js is written; `instanceLimit` only READS `_maxInstanceCount` for probes and tests, so a three.js
// upgrade that drops the field makes the tests fail loudly instead of the game hiding instances.

import {
  DynamicDrawUsage,
  InstancedBufferAttribute,
  InstancedBufferGeometry,
  type Mesh,
  type TypedArray,
} from 'three';

/** A per-instance attribute of `capacity` items of `itemSize` floats (dynamic draw: rewritten every compaction). */
export function createInstancedAttribute(
  capacity: number,
  itemSize: number,
): InstancedBufferAttribute {
  const a = new InstancedBufferAttribute(new Float32Array(capacity * itemSize), itemSize);
  a.setUsage(DynamicDrawUsage);
  return a;
}

/**
 * Releases an instanced geometry: deletes every attribute that is not an `InstancedBufferAttribute` and the index (they belong
 * to a source geometry, the `ModelCache` blocks or a mover's own mesh, and stay alive there), then `dispose()`s it, so three.js
 * frees only the instance buffers, the VAOs and the cached instance limit.
 */
export function releaseInstancedGeometry(g: InstancedBufferGeometry): void {
  for (const name of Object.keys(g.attributes)) {
    if (!isInstanced(g.attributes[name])) g.deleteAttribute(name);
  }
  g.setIndex(null);
  g.dispose();
}

/**
 * Grows the per-instance attributes of `mesh.geometry` (an `InstancedBufferGeometry`) to `capacity` items: builds a NEW
 * geometry that shares (same objects) every non-instanced attribute and the index of the old one, copies its `drawRange`,
 * `groups`, `name`, `instanceCount` and clones of `boundingSphere`/`boundingBox`, replaces every `InstancedBufferAttribute` by
 * a new one of `capacity` items (same name, array type, `itemSize`, `normalized`, `meshPerAttribute`, `gpuType` and usage;
 * `keepData` copies the old contents to the start), assigns it to `mesh.geometry`, releases the old geometry with
 * `releaseInstancedGeometry` and returns the new one (callers re-read their attributes with `next.getAttribute(name)`).
 *
 * `capacity` must be larger than the current one (the `count` of the first instanced attribute of the old geometry): callers
 * grow by doubling. Morph attributes are not supported.
 */
export function growInstanced(
  mesh: Mesh,
  capacity: number,
  keepData: boolean,
): InstancedBufferGeometry {
  const old = mesh.geometry;
  if (!isInstancedGeometry(old))
    throw new Error('growInstanced: the mesh has no instanced geometry');
  if (Object.keys(old.morphAttributes).length > 0) {
    throw new Error('growInstanced: morph attributes are not supported');
  }
  let current = -1;
  for (const name of Object.keys(old.attributes)) {
    const a = old.attributes[name];
    if (isInstanced(a)) {
      current = a.count;
      break;
    }
  }
  if (current < 0) throw new Error('growInstanced: the geometry has no instanced attribute');
  if (capacity <= current) {
    throw new Error(
      `growInstanced: capacity ${String(capacity)} is not larger than the current ${String(current)}`,
    );
  }
  const next = new InstancedBufferGeometry();
  for (const name of Object.keys(old.attributes)) {
    const a = old.attributes[name];
    if (!a) continue;
    if (!isInstanced(a)) {
      next.setAttribute(name, a);
      continue;
    }
    const Ctor = a.array.constructor as new (length: number) => TypedArray;
    const grown = new InstancedBufferAttribute(
      new Ctor(capacity * a.itemSize),
      a.itemSize,
      a.normalized,
      a.meshPerAttribute,
    );
    grown.setUsage(a.usage);
    grown.gpuType = a.gpuType;
    if (keepData) grown.array.set(a.array);
    next.setAttribute(name, grown);
  }
  if (old.index) next.setIndex(old.index);
  next.drawRange.start = old.drawRange.start;
  next.drawRange.count = old.drawRange.count;
  next.groups = old.groups.map((g) => ({ ...g }));
  next.name = old.name;
  next.instanceCount = old.instanceCount;
  next.boundingSphere = old.boundingSphere ? old.boundingSphere.clone() : null;
  next.boundingBox = old.boundingBox ? old.boundingBox.clone() : null;
  mesh.geometry = next;
  releaseInstancedGeometry(old);
  return next;
}

/**
 * The instance limit three.js cached at the first draw of `g` (`_maxInstanceCount`, see the header), `Infinity` before it.
 * Read only, for probes and tests.
 */
export function instanceLimit(g: InstancedBufferGeometry): number {
  return (g as unknown as { _maxInstanceCount?: number })._maxInstanceCount ?? Infinity;
}

/** `true` when three.js would draw fewer instances than `g.instanceCount` asks for (instances silently missing). */
export function exceedsInstanceLimit(g: InstancedBufferGeometry): boolean {
  return g.instanceCount > instanceLimit(g);
}

function isInstanced(a: unknown): a is InstancedBufferAttribute {
  return a instanceof InstancedBufferAttribute;
}

function isInstancedGeometry(g: unknown): g is InstancedBufferGeometry {
  return g instanceof InstancedBufferGeometry;
}
