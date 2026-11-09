// The `.sfx` source over doc 40's asset manager (Plan/40 section 9, Plan/41 section 22.1 `SfxType`): the `sfx` decode job of the
// worker pool, ref-counted handles held for the life of the source (`release()` at renderer disposal). The prototypes are tiny
// (3 MB for the whole corpus); a missing or corrupt file rejects and the prototype cache remembers it as missing (KQ-30).

import type { AssetHandle } from '../../assets/handles';
import type { AssetManager } from '../../assets/manager';
import type { AssetType } from '../../assets/types';
import type { SfxMeta } from '../../gen/formats';
import type { SfxSource } from './SfxPrototypes';

/** The decoded `.sfx` (the pack has no blob: `SfxMeta` is everything). */
export const SfxMetaType: AssetType<SfxMeta> = {
  name: 'sfx',
  decode: 'sfx',
  cpuKeep: true,
  toValue: (result) => (result as { meta: SfxMeta }).meta,
  // a rough size for the manager's CPU budget (the JSON-shaped description: parts, keys, strings)
  cpuBytes: (v) => 512 + v.parts.reduce((n, p) => n + 256 + p.keys.length * 160, 0),
};

export interface ManagerSfxSource extends SfxSource {
  /** Handles currently held (probe). */
  readonly held: number;
  /** Releases every handle (renderer disposal). */
  release(): void;
}

export function createManagerSfxSource(manager: Pick<AssetManager, 'acquire'>): ManagerSfxSource {
  const handles: AssetHandle<SfxMeta>[] = [];
  return {
    async load(file, prio) {
      const handle = manager.acquire(SfxMetaType, { ns: 'res', path: file }, prio, 0, {
        scoped: false,
      });
      handles.push(handle);
      return await handle.ready;
    },
    get held() {
      return handles.length;
    },
    release() {
      for (const h of handles) h.release();
      handles.length = 0;
    },
  };
}
