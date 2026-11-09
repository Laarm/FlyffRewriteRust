// The world renderer's ports over doc 40's asset manager (Plan/41 section 22, Plan/40 section 9, WP-REND-16).
//
//   `landAssetType(mpu)`   `.lnd` -> the `lnd` decode job (placement from the FILE NAME) -> `{ meta, blob }`
//   `StaticModelType`      `.o3d` -> the `o3d` job (`Space::Three`, all LOD groups, name check of KQ-42)
//   `createManagerPorts`   `LandSource` / `ModelSource` / `RawFileSource` that acquire these types (scoped to the world
//                          generation: `bumpGeneration()` on a world change cancels queued loads and drops in-flight results)
//
// The handle of a decoded land or model is held until the renderer evicts it (`DecodedLand.release`), which is what lets the
// manager's refcounts, its CPU/GPU budgets and its idle eviction see the world's assets. Failures map to the renderer's
// vocabulary: a missing file (`notFound`/`unauthorized`/`bad`) is an `AssetNotFoundError` (the object is invisible, KQ-30).

import type { AssetHandle } from '../../assets/handles';
import { isAssetError } from '../../assets/errors';
import type { AssetManager } from '../../assets/manager';
import type { AssetKey } from '../../assets/source';
import type { AssetType } from '../../assets/types';
import type { LndMeta, O3dMeta } from '../../gen/formats';
import type { Prio } from '../../workers/priority';
import { landDecodeOptions, meshDecodeOptions } from '../assetTypes';
import {
  AssetNotFoundError,
  landPath,
  type DecodedLand,
  type DecodedModel,
  type LandSource,
  type ModelSource,
  type RawFileSource,
} from './ports';

interface DecodeResultLike<M> {
  readonly meta: M;
  readonly blob: ArrayBuffer | null;
}

/** The asset type of a land of a world with `mpu` units per cell (the mpu is part of the cache identity). */
export function landAssetType(mpu: number): AssetType<DecodedLand> {
  return {
    name: `land@${String(mpu)}`,
    decode: (key) => {
      const opts = key.ns === 'res' ? landDecodeOptions(key.path, mpu) : null;
      return opts ? { kind: 'lnd', opts } : null;
    },
    // the heights stay (the CPU height field and the placements read the blob)
    cpuKeep: true,
    toValue(result): DecodedLand {
      const r = result as DecodeResultLike<LndMeta>;
      if (!r.blob) throw new Error('the lnd decoder returned no data');
      return { meta: r.meta, blob: new Uint8Array(r.blob) };
    },
    cpuBytes: (v) => v.blob.byteLength,
  };
}

/** The asset type of a static object model: the `MeshType` job with its decode options. */
export const StaticModelType: AssetType<DecodedModel> = {
  name: 'static-model',
  decode: (key) => {
    return key.ns === 'res' ? { kind: 'o3d', opts: meshDecodeOptions(key.path) } : null;
  },
  cpuKeep: true,
  toValue(result): DecodedModel {
    const r = result as DecodeResultLike<O3dMeta>;
    if (!r.blob) throw new Error('the o3d decoder returned no data');
    return { meta: r.meta, blob: new Uint8Array(r.blob) };
  },
  cpuBytes: (v) => v.blob.byteLength,
};

function mapError(e: unknown, path: string): unknown {
  if (isAssetError(e) && e.missing) return new AssetNotFoundError(path);
  return e;
}

async function acquireValue<T>(
  handle: AssetHandle<T>,
  path: string,
  signal: AbortSignal | undefined,
): Promise<{ value: T; release: () => void }> {
  const release = (): void => {
    handle.release();
  };
  // a cancelled request gives its reference back at once: the manager cancels a load nobody waits for
  const onAbort = (): void => {
    release();
  };
  signal?.addEventListener('abort', onAbort, { once: true });
  try {
    const value = await handle.ready;
    return { value, release };
  } catch (e) {
    release();
    throw mapError(e, path);
  } finally {
    signal?.removeEventListener('abort', onAbort);
  }
}

/** The land and model ports (and raw files) over `manager`, scoped to the world generation. */
export function createManagerPorts(manager: AssetManager): {
  readonly lands: LandSource;
  readonly models: ModelSource;
  readonly files: RawFileSource;
} {
  const types = new Map<number, AssetType<DecodedLand>>();
  const landType = (mpu: number): AssetType<DecodedLand> => {
    let t = types.get(mpu);
    if (!t) {
      t = landAssetType(mpu);
      types.set(mpu, t);
    }
    return t;
  };
  return {
    lands: {
      async load(req) {
        const path = landPath(req.folder, req.x, req.z);
        const key: AssetKey = { ns: 'res', path };
        const handle = manager.acquire(landType(req.mpu), key, req.prio, req.distance, {
          scoped: true,
        });
        const { value, release } = await acquireValue(handle, path, req.signal);
        return { meta: value.meta, blob: value.blob, release };
      },
    },
    models: {
      async load(path: string, prio: Prio, distance: number, signal?: AbortSignal) {
        const key: AssetKey = { ns: 'res', path };
        const handle = manager.acquire(StaticModelType, key, prio, distance, { scoped: true });
        const { value, release } = await acquireValue(handle, path, signal);
        return { meta: value.meta, blob: value.blob, release };
      },
    },
    files: {
      async fetch(path, prio, signal) {
        try {
          return await manager.fetchBytes({ ns: 'res', path }, prio, {
            scoped: true,
            ...(signal ? { signal } : {}),
          });
        } catch (e) {
          throw mapError(e, path);
        }
      },
    },
  };
}
