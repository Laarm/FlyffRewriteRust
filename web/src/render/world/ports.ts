// The asset ports of the world renderer (Plan/41 section 22, Plan/40 section 9, WP-REND-16). The renderer never fetches
// anything itself: it asks these narrow interfaces, which doc 40's asset manager implements in the game (fetch scheduler,
// de-duplication, retries, the decode worker pool, `Prio` classes) and the dev pages implement over plain `fetch`.
//
//   `RawFileSource`   a file of the game data (`/res/<fp>/<vfs path>`)
//   `LandSource`      fetch + decode of one `.lnd` (the `lnd` job of the decode pool)
//   `ModelSource`     fetch + decode of one `.o3d` (the `o3d` job, Space::Three)
//   `WorldRegistry`   the client data tables the renderer needs: terrain texture files and water sets (`terrain` bundle),
//                     the static model registry (`models` bundle, object type 0)
//
// The registry is built from the bundles with `createWorldRegistry`.

import type { LndMeta, O3dMeta } from '../../gen/formats';
import type { ModelRow, ModelsBundle, TerrainBundle } from '../../gen/bundles/types';
import { decodeRows } from '../../data/rowTable';
import type { Prio } from '../../workers/priority';

/** The file does not exist on the server (404 / not client-visible): the asset is missing (KQ-30), never retried. */
export class AssetNotFoundError extends Error {
  constructor(readonly path: string) {
    super(`${path} not found`);
    this.name = 'AssetNotFoundError';
  }
}

/** Raw bytes of a game file. */
export interface RawFileSource {
  /** Rejects with `AssetNotFoundError` when the server has no such file. */
  fetch(path: string, prio: Prio, signal?: AbortSignal): Promise<ArrayBuffer>;
}

export interface LandRequest {
  readonly folder: string;
  readonly x: number;
  readonly z: number;
  readonly mpu: number;
  readonly prio: Prio;
  /** Distance of the land from the player (lower first inside a class). */
  readonly distance: number;
  readonly signal?: AbortSignal;
}

export interface DecodedLand {
  readonly meta: LndMeta;
  readonly blob: Uint8Array;
  /** Releases the asset manager's handle of the decoded land (called when the land is evicted); optional. */
  readonly release?: () => void;
}

export interface LandSource {
  /** Rejects with `AssetNotFoundError` for a land without a file; other errors are decode or network failures. */
  load(req: LandRequest): Promise<DecodedLand>;
}

export interface DecodedModel {
  readonly meta: O3dMeta;
  readonly blob: Uint8Array;
  /** Releases the asset manager's handle (called when no land uses the model any more); optional. */
  readonly release?: () => void;
}

export interface ModelSource {
  /** Rejects with `AssetNotFoundError` for a missing file (the object is skipped, KQ-30). */
  load(path: string, prio: Prio, distance: number, signal?: AbortSignal): Promise<DecodedModel>;
}

/** The land file path of land `(x, z)`: `world/<folder>/<folder>XX-ZZ.lnd` with `%02d-%02d` (X first, then Z). */
export function landPath(folder: string, x: number, z: number): string {
  const f = folder.toLowerCase();
  const two = (n: number): string => String(n).padStart(2, '0');
  return `world/${f}/${f}${two(x)}-${two(z)}.lnd`;
}

/** One static object model of the registry (`models.types["0"]`, Plan/41 11.1). */
export interface ObjectModelInfo {
  readonly index: number;
  /** VFS path, `null` when the file does not exist (the object is skipped). */
  readonly file: string | null;
  readonly name: string;
  /** `MODELTYPE_*`: 1 mesh, 2 animated mesh, 3 billboard, 4 sfx. */
  readonly modelType: number;
  /** `MD_*` distance class: 0 far, 1 mid, 2 near, 3 fix. */
  readonly distant: number;
  readonly pick: boolean;
  /** The default scale of the registry entry (the placement scale multiplies the matrix, not this). */
  readonly scale: number;
  /** Fades when it hides the player (`m_bTrans`). */
  readonly trans: boolean;
  readonly shadow: boolean;
  /** `ATEX_*` texture animation / texture set. */
  readonly textureEx: number;
  readonly renderFlag: number;
}

export interface WorldRegistry {
  /** VFS path of terrain texture `id` (`world/texture/<file>`), `null` for an unknown id. */
  terrainFile(id: number): string | null;
  /** Terrain texture ids of the frames of water set `set` in file order (empty for an unknown set). */
  waterFrames(set: number): readonly number[];
  /** Number of water sets. */
  readonly waterSetCount: number;
  /** The registry entry of static object `index` (`mdlObj`), `null` for an unknown index. */
  objectModel(index: number): ObjectModelInfo | null;
}

/** The terrain texture path of a bundle row (`file` is a bare file name under `world/texture/`). */
export function terrainTexturePath(file: string): string {
  return `world/texture/${file.toLowerCase()}`;
}

/**
 * Builds the registry from the bundles. Water sets are the sub-groups of the top-level `Terrain.inc` group with a
 * non-zero frame count (`"물" 2`), in file order (`"물1" 54` = set 0 = river, `"물2" 54` = set 1 = Harmonin); the frame
 * ids of a set are the rows of that sub-group in file order (ref/19 section 14, Plan/41 10.1).
 */
export function createWorldRegistry(terrain: TerrainBundle, models: ModelsBundle): WorldRegistry {
  const files = new Map<number, string>();
  const subgroups: string[] = [];
  const frames = new Map<string, number[]>();
  for (const t of terrain.textures) {
    files.set(t.id, terrainTexturePath(t.file));
    const top = t.group_path[0];
    const sub = t.group_path[1];
    if (top && top[1] > 0 && sub) {
      // a frame of a water set: the sub-group is the set
      const name = sub[0];
      if (!frames.has(name)) {
        frames.set(name, []);
        subgroups.push(name);
      }
      frames.get(name)?.push(t.id);
    }
  }
  const objRows = models.types['0'];
  const objects = new Map<number, ObjectModelInfo>();
  if (objRows) {
    for (const r of decodeRows<ModelRow>(objRows)) {
      objects.set(r.index, {
        index: r.index,
        file: r.file,
        name: r.name,
        modelType: r.model_type,
        distant: r.distant,
        pick: r.pick,
        scale: r.scale,
        trans: r.trans,
        shadow: r.shadow,
        textureEx: r.texture_ex,
        renderFlag: r.render_flag,
      });
    }
  }
  return {
    terrainFile: (id) => files.get(id) ?? null,
    waterFrames: (set) => {
      const name = subgroups[set];
      return name === undefined ? [] : (frames.get(name) ?? []);
    },
    waterSetCount: subgroups.length,
    objectModel: (index) => objects.get(index) ?? null,
  };
}

/** Everything the world renderer needs from the outside. */
export interface WorldAssets {
  readonly lands: LandSource;
  readonly models: ModelSource;
  readonly registry: WorldRegistry;
}
