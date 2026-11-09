// The decode workers of the map (Plan/25 section 3.4, 3.8): the DDS tiles of the original minimap go through the same worker pool and WASM
// decoders as the game (`workers/pool.ts`, job `dds`; without `WEBGL_compressed_texture_s3tc` the pool runs it as `dds_bc` = RGBA8). The
// pool is created on first use, which is when the first level-0 tile is wanted: a map that only shows overview tiles never starts a worker.
import { compileModule } from '../../wasm/loader';
import { WorkerPool } from '../../workers/pool';

let pool: Promise<WorkerPool> | null = null;
let s3tc = false;

/** Workers of the map: two are plenty for 256 x 256 tiles. */
export const MAP_WORKERS = 2;

/** `true` when the GPU takes BC1 blocks as they are (tells the pool which form of `dds` to produce). */
export function setS3tc(available: boolean): void {
  s3tc = available;
}

/** The shared pool of this console tab, started on first call. A failed start is forgotten so that the next call tries again. */
export function decodePool(): Promise<WorkerPool> {
  pool ??= (async () => {
    const module = await compileModule();
    const { pool: p } = await WorkerPool.create(MAP_WORKERS, module, undefined, { s3tc });
    return p;
  })().catch((e: unknown) => {
    pool = null;
    throw e;
  });
  return pool;
}

/** Stops the workers (the map was hidden for good). */
export function terminateDecodePool(): void {
  const p = pool;
  pool = null;
  if (p)
    void p
      .then((x) => {
        x.terminate();
      })
      .catch(() => undefined);
}
