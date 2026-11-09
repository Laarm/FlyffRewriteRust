// The textures of the sky (Plan/41 section 14.3-14.5, 15; SkyBox.cpp `InitDeviceObjects` 1283-1450), all under `weather/`:
//
//   skybox01/02/03.dds   2048 x 512 DXT1 day / sunset / night            (`_low`: 1024 x 256)
//   cloud01/02/03.dds    512 / 512 / 1024 square DXT1                      (`_low`: half size)
//   Moon.dds, sundisk.bmp, Flare_3.tga, lenzflare_001..008.tga, snow (= sundisk.bmp in this build)
//
// The texture-quality option picks the variants (0 all full, 1 `skybox01_low skybox02_low skybox03` and the same for clouds,
// 2 all low). A world's `.wld` `sky` / `cloud` triple replaces the default triple when its first entry is set (`_kSky[0]._bUse`),
// the `sun` / `moon` names replace the built-in bodies. FIX: the original stores the `moon` key into the SUN slot
// (WorldFile.cpp:185-193); here it replaces the moon. A slot of a world triple that is missing keeps the default texture (the
// original would `assert` and skip the whole sky).
//
// `SkyTextures` acquires the handles lazily through the texture cache with the `sky` sampler (linear, no mips, repeat). A texture
// that is not `ready` yields `null`: the sky never draws the white stand-in of a failed texture.

import type { Texture } from 'three';
import { Prio } from '../../workers/priority';
import type { TextureCache, TextureHandle } from '../gl/textures';
import { skyTextureNames, type SkyKind } from './schedule';

/** The part of the world settings the sky needs. */
export interface SkySettings {
  readonly sky: readonly (string | null)[];
  readonly cloud: readonly (string | null)[];
  readonly sun: string | null;
  readonly moon: string | null;
}

export const WEATHER_DIR = 'weather/';

/** The resolved texture paths (lower-case VFS paths). */
export interface SkyPaths {
  /** `[day, sunset, night]`. */
  readonly sky: readonly [string, string, string];
  readonly cloud: readonly [string, string, string];
  readonly sun: string;
  readonly moon: string;
  readonly glow: string;
  /** The snow flake texture (`sundisk.bmp`). */
  readonly snow: string;
  /** The 7 lens flare sprites in DRAW order: `lenzflare_007`, `_006`, ... `_001`. */
  readonly flares: readonly string[];
}

const KIND_INDEX: Readonly<Record<SkyKind, 0 | 1 | 2>> = { day: 0, sunset: 1, night: 2 };

const triple = (
  defaults: readonly [string, string, string],
  world: readonly (string | null)[],
): [string, string, string] => {
  const use = (world[0] ?? null) !== null;
  const pick = (i: 0 | 1 | 2): string => (use ? (world[i] ?? defaults[i]) : defaults[i]);
  return [pick(0), pick(1), pick(2)];
};

/** The paths for a world and a texture-quality option. */
export function resolveSkyPaths(settings: SkySettings, quality: 0 | 1 | 2): SkyPaths {
  const names = skyTextureNames(quality);
  const full = (n: readonly [string, string, string]): [string, string, string] => [
    `${WEATHER_DIR}${n[0]}`,
    `${WEATHER_DIR}${n[1]}`,
    `${WEATHER_DIR}${n[2]}`,
  ];
  const sunDefault = `${WEATHER_DIR}sundisk.bmp`;
  return {
    sky: triple(full(names.sky), settings.sky),
    cloud: triple(full(names.cloud), settings.cloud),
    sun: settings.sun ?? sunDefault,
    moon: settings.moon ?? `${WEATHER_DIR}moon.dds`,
    glow: `${WEATHER_DIR}flare_3.tga`,
    snow: sunDefault,
    flares: [7, 6, 5, 4, 3, 2, 1].map((n) => `${WEATHER_DIR}lenzflare_00${String(n)}.tga`),
  };
}

/** The path of the layer `kind` of the sky cylinder / cloud dome in `paths`. */
export function layerPath(paths: SkyPaths, which: 'sky' | 'cloud', kind: SkyKind): string {
  return paths[which][KIND_INDEX[kind]];
}

/** The lazily acquired texture handles of one world's sky. */
export class SkyTextures {
  private readonly handles = new Map<string, TextureHandle>();
  private cachedQuality: 0 | 1 | 2 | null = null;
  private cachedPaths: SkyPaths | null = null;
  private disposed = false;

  constructor(
    private readonly cache: Pick<TextureCache, 'acquire'>,
    private readonly settings: SkySettings,
    private readonly quality: () => 0 | 1 | 2,
  ) {}

  /** The paths for the current texture-quality option (re-resolved when it changed). */
  paths(): SkyPaths {
    const q = this.quality();
    if (this.cachedPaths === null || this.cachedQuality !== q) {
      this.cachedQuality = q;
      this.cachedPaths = resolveSkyPaths(this.settings, q);
      // textures of the previous variant are not needed any more
      const keep = new Set<string>([
        ...this.cachedPaths.sky,
        ...this.cachedPaths.cloud,
        this.cachedPaths.sun,
        this.cachedPaths.moon,
        this.cachedPaths.glow,
        this.cachedPaths.snow,
        ...this.cachedPaths.flares,
      ]);
      for (const [path, h] of [...this.handles]) {
        if (!keep.has(path)) {
          h.release();
          this.handles.delete(path);
        }
      }
    }
    return this.cachedPaths;
  }

  /** Starts loading `path` (idempotent). */
  request(path: string, prio: Prio): TextureHandle | null {
    if (this.disposed) return null;
    let h = this.handles.get(path);
    if (!h) {
      h = this.cache.acquire(path, 'sky', { prio });
      this.handles.set(path, h);
    }
    return h;
  }

  /** The GPU texture of `path` once it is `ready`, else `null` (also for a failed load). */
  texture(path: string, prio: Prio = Prio.Near): Texture | null {
    const h = this.request(path, prio);
    return h?.state === 'ready' ? h.texture : null;
  }

  /** `true` when `path` is requested and still loading. */
  loading(path: string): boolean {
    return this.handles.get(path)?.state === 'loading';
  }

  /** Paths requested so far with their state (probe, tests). */
  states(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const [p, h] of this.handles) out[p] = h.state;
    return out;
  }

  dispose(): void {
    this.disposed = true;
    for (const h of this.handles.values()) h.release();
    this.handles.clear();
  }
}
