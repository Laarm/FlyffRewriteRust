// The asset types of the sim inputs (Plan/40 section 9.4, WP-CLIENT-18/21): the `chr` and `ani` decode jobs whose blob is the
// `FSK1` / `FMO1` pack that `SimWorld.add_skeleton` / `add_clip` take UNCHANGED (Plan/30 13.4), and the collision chunk
// (`chunk_check` job: the `FLYC` bytes validated in a worker and returned as a fresh transferable copy).
//
// The decoded `.chr` is the same asset 41 reads the bind matrices from (`SkeletonType`: `space: 'three'`), so a skeleton is
// fetched and decoded once for the renderer and the sim; the cache identity is `<type name>|<key>`, which is why both consumers
// acquire THIS type.

import type { AniMeta, ChrMeta } from '../gen/formats';
import type { AssetType } from './types';

/** A decoded pack: the meta of the decoder and its blob (the sim input). */
export interface PackValue<M> {
  readonly meta: M;
  readonly blob: Uint8Array;
}

interface DecodeResultLike<M> {
  readonly meta: M;
  readonly blob: ArrayBuffer | null;
}

function pack<M>(result: unknown, what: string): PackValue<M> {
  const r = result as DecodeResultLike<M>;
  if (!r.blob) throw new Error(`the ${what} decoder returned no blob`);
  return { meta: r.meta, blob: new Uint8Array(r.blob) };
}

/** `.chr` skeleton: `ChrMeta` (three.js space grips and events) + the `FSK1` blob; kept for the renderer's bind matrices. */
export const ChrPackType: AssetType<PackValue<ChrMeta>> = {
  name: 'skeleton',
  decode: () => ({ kind: 'chr', opts: { space: 'three' } }),
  cpuKeep: true,
  toValue: (result) => pack<ChrMeta>(result, 'chr'),
  cpuBytes: (v) => v.blob.byteLength,
};

/** `.ani` clip: `AniMeta` + the `FMO1` blob. */
export const AniPackType: AssetType<PackValue<AniMeta>> = {
  name: 'clip',
  decode: 'ani',
  cpuKeep: false,
  toValue: (result) => pack<AniMeta>(result, 'ani'),
  cpuBytes: (v) => v.blob.byteLength,
};

/** One `FLYC` collision chunk (`gen:collision:<folder>/<x>-<z>`), validated by the `chunk_check` worker job. */
export const ChunkType: AssetType<Uint8Array> = {
  name: 'collision-chunk',
  decode: 'chunk_check',
  cpuKeep: false,
  toValue: (result) => {
    const r = result as { blob: ArrayBuffer | null };
    if (!r.blob) throw new Error('the chunk check returned no bytes');
    return new Uint8Array(r.blob);
  },
  cpuBytes: (v) => v.byteLength,
};
