// The SFX textures of the renderer (Plan/41 section 16.1, 22.1 `SfxTextureType`): canonical path -> small integer id, backed by the
// texture cache's `sfx` sampler and RGBA5551 conversion. An id is DRAWABLE only while its texture is resident: a texture that is
// still loading, lost with the context or failed (6 referenced names do not exist, KQ-30) draws nothing - never a white or black
// quad. The handles are held for the life of the system (35 MB of RGBA5551 at most for the whole corpus).

import type { Texture } from 'three';
import { Prio } from '../../workers/priority';
import type { TextureHandle } from '../gl/textures';

/** What the SFX textures need of the texture cache. */
export interface SfxTextureCache {
  acquire(
    path: string,
    preset: 'sfx',
    options: { sfx: true; prio: Prio; distance?: number },
  ): TextureHandle;
}

export class SfxTextures {
  private readonly ids = new Map<string, number>();
  private readonly handles: TextureHandle[] = [];
  private readonly paths: string[] = [];

  constructor(private readonly cache: SfxTextureCache) {}

  /** The id of `path`, acquiring the texture on first use. */
  idOf(path: string): number {
    let id = this.ids.get(path);
    if (id === undefined) {
      id = this.handles.length;
      this.ids.set(path, id);
      this.handles.push(this.cache.acquire(path, 'sfx', { sfx: true, prio: Prio.Fx }));
      this.paths.push(path);
    }
    return id;
  }

  /** The id of `path` when its texture can be drawn now, else `-1` (the part draws nothing). */
  drawable(path: string): number {
    const id = this.idOf(path);
    return this.handles[id]?.state === 'ready' ? id : -1;
  }

  /** `loading` (or lost), `ready` or `failed` (missing file, decode error: nothing is ever drawn). */
  stateOf(path: string): 'loading' | 'ready' | 'failed' {
    const s = this.handles[this.idOf(path)]?.state;
    return s === 'ready' ? 'ready' : s === 'failed' ? 'failed' : 'loading';
  }

  /** The three.js texture of a drawable id, `null` otherwise. */
  textureOf(id: number): Texture | null {
    const h = this.handles[id];
    return h?.state === 'ready' ? h.texture : null;
  }

  /** The path of an id (probes). */
  pathOf(id: number): string | undefined {
    return this.paths[id];
  }

  get count(): number {
    return this.handles.length;
  }

  /** Textures still loading or lost (probes; the dev page waits for 0). */
  get loading(): number {
    let n = 0;
    for (const h of this.handles) if (h.state === 'loading' || h.state === 'lost') n++;
    return n;
  }

  /** Textures that failed (probes). */
  get failed(): number {
    let n = 0;
    for (const h of this.handles) if (h.state === 'failed') n++;
    return n;
  }

  dispose(): void {
    for (const h of this.handles) h.release();
    this.handles.length = 0;
    this.ids.clear();
    this.paths.length = 0;
  }
}
