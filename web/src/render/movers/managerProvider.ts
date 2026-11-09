// The model provider of the mover visuals over doc 40's asset manager (Plan/40 section 9, Plan/41 section 22; WP-REND-12/16):
//
//   `model(path)`     a mover part or whole model: the `o3d` decode job (`StaticModelType`, `Space::Three`, all LOD groups, the
//                     name check of KQ-42) -> the renderer's `ModelCache` (ref-counted geometry shared with the static objects)
//   `skeleton(path)`  a `.chr`: the SAME `chr` asset the pose feeder hands to the sim (`ChrPackType`), so one fetch and one
//                     decode serve the sim and the renderer; the render skeleton keeps only the bind matrices (`buildSkeletonRender`)
//
// Both are cached per canonical path for the life of the provider. A file the server does not have, or that cannot be decoded
// (KQ-30, KQ-42), answers `null` for good (the part is invisible); a network failure or a cancelled load (a world change)
// is NOT cached, the next request tries again. The handles of the decoded assets are held until `release()` (world change,
// renderer disposal) so that the manager's budgets and its idle eviction see the movers' assets.

import { isAssetError } from '../../assets/errors';
import type { AssetHandle } from '../../assets/handles';
import type { AssetManager } from '../../assets/manager';
import { ChrPackType } from '../../assets/poseTypes';
import { Prio } from '../../workers/priority';
import type { ModelCache, ModelGeometry } from '../objects/ModelCache';
import { StaticModelType } from '../world/managerPorts';
import type { ModelProvider } from './MoverVisual';
import { buildSkeletonRender, type SkeletonRender } from './skeleton';

export interface ManagerProviderDeps {
  readonly manager: Pick<AssetManager, 'acquire'>;
  /** The renderer's geometry cache (`WorldRenderer.models`). */
  readonly models: Pick<ModelCache, 'add' | 'release'>;
  readonly log?: (message: string) => void;
}

export interface ManagerModelProvider extends ModelProvider {
  /** Models and skeletons requested / resident / known missing (probe). */
  stats(): { models: number; skeletons: number; missing: number };
  /** Releases the asset handles and the geometry references (the geometry itself stays under the cache's LRU). */
  release(): void;
}

/** `true` for an error that is final for the file (does not exist for this client, cannot be decoded). */
function isFinal(e: unknown): boolean {
  return isAssetError(e) && (e.missing || e.reason === 'decode');
}

export function createManagerModelProvider(deps: ManagerProviderDeps): ManagerModelProvider {
  const models = new Map<string, Promise<ModelGeometry | null>>();
  const skeletons = new Map<string, Promise<SkeletonRender | null>>();
  const handles: AssetHandle<unknown>[] = [];
  const held: string[] = [];
  const missing = new Set<string>();
  const log = deps.log ?? ((): void => undefined);

  const note = (what: string, path: string, e: unknown): void => {
    missing.add(path);
    log(`${what} ${path}: ${e instanceof Error ? e.message : String(e)}`);
  };

  return {
    model(path, prio: Prio = Prio.Visible) {
      let p = models.get(path);
      if (!p) {
        const handle = deps.manager.acquire(StaticModelType, { ns: 'res', path }, prio, 0, {
          scoped: true,
        });
        handles.push(handle);
        p = handle.ready.then(
          (decoded) => {
            try {
              const geometry = deps.models.add(path, decoded.meta, decoded.blob);
              held.push(path);
              return geometry;
            } catch (e) {
              note('model', path, e); // sections that do not fit the blob (ModelBuildError)
              return null;
            }
          },
          (e: unknown) => {
            // a final failure is remembered; a cancelled or failed transfer is asked for again next time
            if (isFinal(e)) note('model', path, e);
            else models.delete(path);
            return null;
          },
        );
        models.set(path, p);
      }
      return p;
    },
    skeleton(path, prio: Prio = Prio.Visible) {
      let p = skeletons.get(path);
      if (!p) {
        const handle = deps.manager.acquire(ChrPackType, { ns: 'res', path }, prio, 0, {
          scoped: true,
        });
        handles.push(handle);
        p = handle.ready.then(
          (pack) => {
            try {
              return buildSkeletonRender(path, pack.meta, pack.blob);
            } catch (e) {
              note('skeleton', path, e); // a corrupt FSK1: the mover has no pose (and no model)
              return null;
            }
          },
          (e: unknown) => {
            if (isFinal(e)) note('skeleton', path, e);
            else skeletons.delete(path);
            return null;
          },
        );
        skeletons.set(path, p);
      }
      return p;
    },
    stats: () => ({ models: models.size, skeletons: skeletons.size, missing: missing.size }),
    release() {
      for (const h of handles) h.release();
      handles.length = 0;
      for (const k of held) deps.models.release(k);
      held.length = 0;
      models.clear();
      skeletons.clear();
    },
  };
}
