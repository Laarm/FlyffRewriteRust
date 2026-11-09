// The ordered draw list of the SFX pass (Plan/41 section 16.7, 2.4 pass 12, ref/20 section 4.8): the parts of all visible effects in
// the order of the original (effects far to near, parts in file order, particles in list order) as batched runs. It is the CPU
// side of the quad batcher and the sink of `emitModel`; the GL objects are `SfxRenderer`'s.
//
//   quads   one instanced unit quad per BILL part / particle: 10 floats `row0.xyz row1.xyz translation.xyz alpha` in THREE.JS space
//           (the z of the rows and of the translation is negated here: the single LH -> three.js mapping of the SFX math)
//   strips  custom-mesh rings: world positions mirrored, `x y z u v alpha` per vertex, converted to indexed triangles
//   meshes  MESH parts (REND-19): the model, the part and its world matrix
//
// Consecutive quads with the same texture and blend mode become ONE run (one draw call); a strip, a mesh or another texture ends
// the run, so the global order is exactly the draw order of the original. Above `MAX_QUADS` quads the excess is dropped
// (`dropped`): the system emits the effects far to near, so the nearest ones that do not fit are the ones lost - it checks
// `full` and skips the farthest remaining effects instead (`SfxSystem`, Plan/41 section 16.7).

import type { Aff } from './mat';
import type { PartSink } from './parts';
import type { PartProto } from './proto';
import type { SfxModel } from './SfxModel';

/** The budget of Plan/41 section 24. */
export const MAX_QUADS = 50_000;
/** Floats per quad instance. */
export const QUAD_STRIDE = 10;
/** Floats per strip vertex: `x y z u v alpha`. */
export const STRIP_STRIDE = 6;

export interface QuadRun {
  readonly kind: 'quads';
  readonly tex: number;
  readonly additive: boolean;
  /** First quad (index into the quad buffer / `QUAD_STRIDE`). */
  readonly start: number;
  count: number;
}

export interface StripRun {
  readonly kind: 'strips';
  readonly tex: number;
  readonly additive: boolean;
  /** First index of the run in the strip index buffer. */
  readonly indexStart: number;
  indexCount: number;
}

export interface MeshItem {
  readonly kind: 'mesh';
  readonly model: SfxModel;
  readonly part: PartProto;
  /** LH world matrix of the model (12 numbers). */
  readonly world: Float64Array;
  readonly alpha: number;
}

export type DrawItem = QuadRun | StripRun | MeshItem;

export class DrawList implements PartSink {
  readonly items: DrawItem[] = [];
  quadData: Float32Array;
  quads = 0;
  /** Quads that did not fit. */
  dropped = 0;
  stripVerts = new Float32Array(STRIP_STRIDE * 4096);
  stripVertCount = 0;
  stripIndex = new Uint32Array(3 * 4096);
  stripIndexCount = 0;

  constructor(readonly maxQuads = MAX_QUADS) {
    this.quadData = new Float32Array(Math.min(maxQuads, 4096) * QUAD_STRIDE);
  }

  reset(): void {
    this.items.length = 0;
    this.quads = 0;
    this.dropped = 0;
    this.stripVertCount = 0;
    this.stripIndexCount = 0;
  }

  get full(): boolean {
    return this.quads >= this.maxQuads;
  }

  quad(
    tex: number,
    additive: boolean,
    r0x: number,
    r0y: number,
    r0z: number,
    r1x: number,
    r1y: number,
    r1z: number,
    tx: number,
    ty: number,
    tz: number,
    alpha: number,
  ): void {
    if (this.quads >= this.maxQuads) {
      this.dropped++;
      return;
    }
    if ((this.quads + 1) * QUAD_STRIDE > this.quadData.length) {
      const grown = new Float32Array(Math.min(this.maxQuads, this.quads * 2 + 1024) * QUAD_STRIDE);
      grown.set(this.quadData);
      this.quadData = grown;
    }
    const o = this.quads * QUAD_STRIDE;
    const d = this.quadData;
    // three.js space: z negated (S * M * S on rows 0, 1 and the translation row)
    d[o] = r0x;
    d[o + 1] = r0y;
    d[o + 2] = -r0z;
    d[o + 3] = r1x;
    d[o + 4] = r1y;
    d[o + 5] = -r1z;
    d[o + 6] = tx;
    d[o + 7] = ty;
    d[o + 8] = -tz;
    d[o + 9] = alpha / 255;
    const last = this.items[this.items.length - 1];
    if (last?.kind === 'quads' && last.tex === tex && last.additive === additive) {
      last.count++;
    } else {
      this.items.push({ kind: 'quads', tex, additive, start: this.quads, count: 1 });
    }
    this.quads++;
  }

  strip(
    tex: number,
    additive: boolean,
    vertices: Float64Array,
    count: number,
    alpha: number,
  ): void {
    if (count < 3) return;
    const needV = (this.stripVertCount + count) * STRIP_STRIDE;
    if (needV > this.stripVerts.length) {
      const grown = new Float32Array(Math.max(needV, this.stripVerts.length * 2));
      grown.set(this.stripVerts);
      this.stripVerts = grown;
    }
    const needI = this.stripIndexCount + (count - 2) * 3;
    if (needI > this.stripIndex.length) {
      const grown = new Uint32Array(Math.max(needI, this.stripIndex.length * 2));
      grown.set(this.stripIndex);
      this.stripIndex = grown;
    }
    const base = this.stripVertCount;
    const a = alpha / 255;
    for (let i = 0; i < count; i++) {
      const o = (base + i) * STRIP_STRIDE;
      const s = i * 5;
      this.stripVerts[o] = vertices[s] ?? 0;
      this.stripVerts[o + 1] = vertices[s + 1] ?? 0;
      this.stripVerts[o + 2] = -(vertices[s + 2] ?? 0);
      this.stripVerts[o + 3] = vertices[s + 3] ?? 0;
      this.stripVerts[o + 4] = vertices[s + 4] ?? 0;
      this.stripVerts[o + 5] = a;
    }
    this.stripVertCount += count;
    // D3DPT_TRIANGLESTRIP: triangle i is (i, i+1, i+2), odd triangles swap the first two to keep the winding
    const start = this.stripIndexCount;
    let w = start;
    for (let i = 0; i + 2 < count; i++) {
      if (i % 2 === 0) {
        this.stripIndex[w++] = base + i;
        this.stripIndex[w++] = base + i + 1;
      } else {
        this.stripIndex[w++] = base + i + 1;
        this.stripIndex[w++] = base + i;
      }
      this.stripIndex[w++] = base + i + 2;
    }
    this.stripIndexCount = w;
    const last = this.items[this.items.length - 1];
    if (last?.kind === 'strips' && last.tex === tex && last.additive === additive) {
      last.indexCount += w - start;
    } else {
      this.items.push({ kind: 'strips', tex, additive, indexStart: start, indexCount: w - start });
    }
  }

  mesh(model: SfxModel, part: PartProto, world: Aff, alpha: number): void {
    this.items.push({ kind: 'mesh', model, part, world: Float64Array.from(world), alpha });
  }

  /** Draw runs (quad and strip runs; meshes excluded). */
  get runCount(): number {
    let n = 0;
    for (const it of this.items) if (it.kind !== 'mesh') n++;
    return n;
  }
}
