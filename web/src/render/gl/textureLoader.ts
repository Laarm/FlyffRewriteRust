// `TextureLoader` over the decode worker pool (Plan/40 section 8, Plan/41 section 22.1 `TextureType`): fetches the
// bytes of a texture in the form the client uses and decodes them in a worker:
//
//   *.dds                -> `dds` job (BC passthrough; the pool turns it into `dds_bc` when S3TC is missing,
//                           `PoolOptions.s3tc`) or `dds_bc` for SFX textures (always RGBA8)
//   *.tga, *.bmp, *.png  -> the PNG form of the file (`/gen/png`, mode `none`, Plan/24 section 5.2) -> `bitmap` job
//
// Fetching (URL resolution through the asset manifest, fetch scheduler, de-duplication) belongs to doc 40's asset
// source; this module only needs `TextureByteSource`.

import type { Prio } from '../../workers/priority';
import type { WorkerPool } from '../../workers/pool';
import {
  TextureNotFoundError,
  type DecodedTexture,
  type TextureLoader,
  type TextureLoadRequest,
} from './textures';

export type TextureForm = 'dds' | 'png';

export interface TextureByteSource {
  /**
   * The file in the given form. Rejects with `TextureNotFoundError` when the server has no such file (404 or
   * not client-visible).
   */
  fetch(path: string, form: TextureForm, prio: Prio, signal?: AbortSignal): Promise<ArrayBuffer>;
}

/** `dds` for DDS files, `png` for TGA/BMP/PNG (converted by the server), `null` for anything else. */
export function textureFormOf(path: string): TextureForm | null {
  const dot = path.lastIndexOf('.');
  const ext = dot < 0 ? '' : path.slice(dot + 1).toLowerCase();
  if (ext === 'dds') return 'dds';
  if (ext === 'tga' || ext === 'bmp' || ext === 'png') return 'png';
  return null;
}

/** The loader of Plan/41 22.1 over `pool` (a `WorkerPool`, or anything with `run`). */
export function createPoolTextureLoader(
  pool: Pick<WorkerPool, 'run'>,
  source: TextureByteSource,
): TextureLoader {
  return {
    async load(req: TextureLoadRequest): Promise<DecodedTexture> {
      const form = textureFormOf(req.path);
      if (form === null) throw new TextureNotFoundError(req.path);
      const bytes = await source.fetch(req.path, form, req.prio);
      if (form === 'png') {
        const r = await pool.run({
          kind: 'bitmap',
          bytes,
          prio: req.prio,
          distance: req.distance,
        });
        return { kind: 'bitmap', bitmap: r.bitmap };
      }
      const r = await pool.run({
        kind: req.sfx ? 'dds_bc' : 'dds',
        bytes,
        prio: req.prio,
        distance: req.distance,
      });
      if (!r.blob) throw new Error(`${req.path}: decoder returned no pixel data`);
      return { kind: 'dds', meta: r.meta, blob: new Uint8Array(r.blob) };
    },
  };
}
