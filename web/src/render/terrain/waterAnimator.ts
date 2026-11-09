// The animated state of water, cloud sea and the underwater overlay that is GLOBAL to a world (Plan/41 section 10): the
// frame clocks, the frame textures of each water set, the cloud texture and the shared materials. Land meshes only
// reference the materials, so a texture swap or an offset change touches one uniform for all lands.

import { RepeatWrapping, type Texture } from 'three';
import { Prio } from '../../workers/priority';
import type { EnvUniforms } from '../gl/materials/env';
import type { TextureCache, TextureHandle } from '../gl/textures';
import type { WorldRegistry } from '../world/ports';
import {
  CLOUD_TEXTURE_ID,
  CloudScroll,
  UNDERWATER_TEXTURE_ID,
  WaterClock,
  createUnderwaterMaterial,
  createWaterMaterial,
  type WaterMaterial,
} from './water';

export class WaterAnimator {
  readonly clock: WaterClock;
  readonly cloud = new CloudScroll();
  /** One material per water set (created on first use). */
  private readonly waterMaterials = new Map<number, WaterMaterial>();
  private readonly frames = new Map<number, (TextureHandle | null)[]>();
  /** Cloud layer materials: y = 40 and y = 80. */
  readonly cloudMaterials: readonly [WaterMaterial, WaterMaterial];
  readonly underwater = createUnderwaterMaterial();
  private cloudHandle: TextureHandle | null = null;
  private underwaterHandle: TextureHandle | null = null;
  /** QA: a test pattern drawn instead of the river frame (a spec tells a flat tint from the whole texture by it). */
  private underwaterTest: Texture | null = null;
  private disposed = false;

  constructor(
    private readonly registry: WorldRegistry,
    private readonly textures: TextureCache,
    private readonly env: EnvUniforms,
  ) {
    const counts: number[] = [];
    for (let s = 0; s < registry.waterSetCount; s++) counts.push(registry.waterFrames(s).length);
    this.clock = new WaterClock(counts);
    this.cloudMaterials = [
      createWaterMaterial(env, 'cloud-sea-40'),
      createWaterMaterial(env, 'cloud-sea-80'),
    ];
  }

  /** The material of water set `set`; its frame textures start loading on the first request. */
  waterMaterial(set: number): WaterMaterial {
    let m = this.waterMaterials.get(set);
    if (!m) {
      m = createWaterMaterial(this.env, `water-set-${String(set)}`);
      this.waterMaterials.set(set, m);
      const ids = this.registry.waterFrames(set);
      const handles = ids.map((id) => {
        const path = this.registry.terrainFile(id);
        return path ? this.textures.acquire(path, 'world', { prio: Prio.Statics }) : null;
      });
      this.frames.set(set, handles);
    }
    return m;
  }

  /** Starts loading the cloud and underwater textures (called when a land has cloud patches / a world is set). */
  ensureCloud(): void {
    if (this.cloudHandle) return;
    const path = this.registry.terrainFile(CLOUD_TEXTURE_ID);
    if (path) this.cloudHandle = this.textures.acquire(path, 'world', { prio: Prio.Statics });
  }

  ensureUnderwater(): void {
    if (this.underwaterHandle) return;
    const path = this.registry.terrainFile(UNDERWATER_TEXTURE_ID);
    if (path) this.underwaterHandle = this.textures.acquire(path, 'world', { prio: Prio.Statics });
  }

  /** The texture to draw for `set` now: the clock's frame, else the nearest earlier (then later) resident one. */
  frameTexture(set: number): Texture | null {
    const list = this.frames.get(set);
    const idx = this.clock.index(set);
    if (!list || idx < 0) return null;
    const ready = (i: number): Texture | null => {
      const h = list[i];
      return h?.state === 'ready' ? h.texture : null;
    };
    const exact = ready(idx);
    if (exact) return exact;
    for (let i = idx - 1; i >= 0; i--) {
      const t = ready(i);
      if (t) return t;
    }
    for (let i = idx + 1; i < list.length; i++) {
      const t = ready(i);
      if (t) return t;
    }
    return null;
  }

  /** `true` once at least one frame texture of the set is resident. */
  setReady(set: number): boolean {
    return this.frameTexture(set) !== null;
  }

  cloudReady(): boolean {
    return this.cloudHandle?.state === 'ready';
  }

  /** The cosmetic 60 Hz steps. */
  tick(steps: number): void {
    this.clock.tick(steps);
    this.cloud.tick(steps);
  }

  /** Per drawn frame: points the materials at the current textures and offsets. */
  update(): void {
    for (const [set, m] of this.waterMaterials) m.uniforms.uMap.value = this.frameTexture(set);
    const cloudTex = this.cloudHandle?.state === 'ready' ? this.cloudHandle.texture : null;
    for (const layer of [0, 1] as const) {
      const m = this.cloudMaterials[layer];
      m.uniforms.uMap.value = cloudTex;
      const [u, v] = this.cloud.offset(layer);
      m.uniforms.uUvOffset.value.set(u, v);
    }
    const uw = this.underwaterHandle;
    this.underwater.uniforms.uMap.value =
      this.underwaterTest ?? (uw?.state === 'ready' ? uw.texture : null);
  }

  underwaterReady(): boolean {
    return this.underwaterTest !== null || this.underwaterHandle?.state === 'ready';
  }

  /**
   * QA hook of `/dev/world` (`underwaterPattern`): draws `texture` (repeat wrap) as the overlay texture instead of the river
   * frame; `null` goes back. Never used by the game.
   */
  setUnderwaterTestTexture(texture: Texture | null): void {
    if (texture) {
      texture.wrapS = RepeatWrapping;
      texture.wrapT = RepeatWrapping;
      texture.needsUpdate = true;
    }
    this.underwaterTest = texture;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const list of this.frames.values()) for (const h of list) h?.release();
    this.cloudHandle?.release();
    this.underwaterHandle?.release();
    for (const m of this.waterMaterials.values()) m.dispose();
    for (const m of this.cloudMaterials) m.dispose();
    this.underwater.dispose();
    this.frames.clear();
    this.waterMaterials.clear();
  }
}
