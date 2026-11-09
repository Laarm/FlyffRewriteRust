// `LandSource` and `ModelSource` over the decode worker pool (Plan/40 section 8, Plan/41 section 22.1 `LandType`/`MeshType`).
// The bytes come from a `RawFileSource` (doc 40's asset source in the game, `fetch` in the dev pages); the decoders run in
// the workers (`lnd`: `decode_lnd` with the land placement of the FILE NAME, `o3d`: `Space::Three` with all LOD groups).

import type { WorkerPool } from '../../workers/pool';
import { meshDecodeOptions } from '../assetTypes';
import {
  landPath,
  type DecodedLand,
  type DecodedModel,
  type LandRequest,
  type LandSource,
  type ModelSource,
  type RawFileSource,
} from './ports';
import type { Prio } from '../../workers/priority';

/** The decode job options of a land (`LandType` of Plan/41 22.1): the placement comes from the FILE NAME, never the header. */
export function landDecodeOptions(req: Pick<LandRequest, 'x' | 'z' | 'mpu'>): {
  land_x: number;
  land_z: number;
  mpu: number;
  normals: false;
} {
  return { land_x: req.x, land_z: req.z, mpu: req.mpu, normals: false };
}

export function createPoolLandSource(
  pool: Pick<WorkerPool, 'run'>,
  files: RawFileSource,
): LandSource {
  return {
    async load(req: LandRequest): Promise<DecodedLand> {
      const bytes = await files.fetch(landPath(req.folder, req.x, req.z), req.prio, req.signal);
      const r = await pool.run({
        kind: 'lnd',
        bytes,
        prio: req.prio,
        distance: req.distance,
        opts: landDecodeOptions(req),
        ...(req.signal ? { signal: req.signal } : {}),
      });
      if (!r.blob)
        throw new Error(`${landPath(req.folder, req.x, req.z)}: the decoder returned no data`);
      return { meta: r.meta, blob: new Uint8Array(r.blob) };
    },
  };
}

export function createPoolModelSource(
  pool: Pick<WorkerPool, 'run'>,
  files: RawFileSource,
  /** `false` keeps the legacy parity of `MeshType` (6 files whose header name differs are rejected). */
  lenient = false,
): ModelSource {
  return {
    async load(
      path: string,
      prio: Prio,
      distance: number,
      signal?: AbortSignal,
    ): Promise<DecodedModel> {
      const bytes = await files.fetch(path, prio, signal);
      const opts = {
        ...meshDecodeOptions(path),
        ...(lenient ? { reject_name_mismatch: false } : {}),
      };
      const r = await pool.run({
        kind: 'o3d',
        bytes,
        prio,
        distance,
        opts,
        ...(signal ? { signal } : {}),
      });
      if (!r.blob) throw new Error(`${path}: the decoder returned no mesh data`);
      return { meta: r.meta, blob: new Uint8Array(r.blob) };
    },
  };
}
