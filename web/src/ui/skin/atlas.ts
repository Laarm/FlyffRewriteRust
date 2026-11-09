// Texture packs (`.inc` sprite sheets, the `ui-atlases` bundle; Plan/42 §6.2, ref/22 §6.9): sprite rectangles in
// pixels and the CPU colour-key pass for atlases whose key is neither "none" nor magenta.

import type { AtlasDto, AtlasSprite, UiAtlasesBundle } from '../../gen/bundles';
import { MISSING, normalizePath, type UiImageCache } from './images';
import { cloneImage, type PixelImage } from './pixels';

/** `0xFFFF00FF`: the colour key the `Exact` PNG form already applied. */
export const MAGENTA_KEY = 0xffff00ff;

/** A sprite of an atlas: the texture, the pixel rectangle and the draw flags. */
export interface AtlasSpriteRef {
  /** The (keyed) texture image the rectangle lies in. */
  readonly img: PixelImage;
  readonly sx: number;
  readonly sy: number;
  readonly w: number;
  readonly h: number;
  /** 0 none, 1 horizontal, 2 vertical, 3 both (`flip` of the `.inc`). */
  readonly flip: number;
  /** The draw centre of a `serialize` sprite. */
  readonly center: readonly [number, number] | null;
}

/** The pixel rectangle `[sx, sy, w, h]` of a sprite for a texture of `texW x texH` (`datauv` forms use the uv). */
export function spriteRect(
  s: AtlasSprite,
  texW: number,
  texH: number,
): readonly [number, number, number, number] {
  if (s.uv) {
    return [Math.round(s.uv[0] * texW), Math.round(s.uv[1] * texH), s.w, s.h];
  }
  return [s.x, s.y, s.w, s.h];
}

/**
 * Sets alpha 0 on every pixel equal to the colour key `key` (`0xAARRGGBB`, D3DX semantics: a key with alpha 255
 * matches the RGB of an opaque pixel; any other alpha compares the full ARGB). Returns a copy.
 */
export function applyColorKey(src: PixelImage, key: number): PixelImage {
  const out = cloneImage(src);
  const kr = (key >>> 16) & 0xff;
  const kg = (key >>> 8) & 0xff;
  const kb = key & 0xff;
  const ka = (key >>> 24) & 0xff;
  const d = out.data;
  for (let i = 0; i < d.length; i += 4) {
    const a = d[i + 3] ?? 0;
    if (
      (d[i] ?? 0) === kr &&
      (d[i + 1] ?? 0) === kg &&
      (d[i + 2] ?? 0) === kb &&
      (ka === 0xff ? a === 0xff : a === ka)
    ) {
      d[i + 3] = 0;
    }
  }
  return out;
}

/** Access to the 39 UI atlases (`Theme/*.inc`, `Icon/*.inc`, `Sfx/*.inc`). */
export class UiAtlases {
  readonly #bundle: () => UiAtlasesBundle | undefined;
  readonly #images: UiImageCache;
  readonly #keyed = new Map<string, PixelImage>();

  /** @param bundle the parsed bundle, or a supplier of it (`UiHost.bundles.atlases` is `undefined` until loaded) */
  constructor(bundle: UiAtlasesBundle | (() => UiAtlasesBundle | undefined), images: UiImageCache) {
    this.#bundle = typeof bundle === 'function' ? bundle : () => bundle;
    this.#images = images;
  }

  /** The canonical paths of every atlas (none while the bundle is not loaded). */
  paths(): string[] {
    return Object.keys(this.#bundle()?.atlases ?? {});
  }

  /** The descriptor of an atlas (path of the `.inc`, case-insensitive). */
  info(incPath: string): AtlasDto | undefined {
    return this.#bundle()?.atlases[normalizePath(incPath)];
  }

  /** The sprite `index` of an atlas, with its (keyed) texture loaded; `undefined` when absent or missing. */
  async sprite(incPath: string, index: number): Promise<AtlasSpriteRef | undefined> {
    const dto = this.info(incPath);
    const s = dto?.sprites[index];
    if (!dto || !s || dto.texture === '') return undefined;
    const img = await this.#texture(dto);
    if (!img) return undefined;
    const [sx, sy, w, h] = spriteRect(s, img.width, img.height);
    return { img, sx, sy, w, h, flip: s.flip, center: s.center ?? null };
  }

  async #texture(dto: AtlasDto): Promise<PixelImage | undefined> {
    const cached = this.#keyed.get(dto.texture);
    if (cached) return cached;
    const img = await this.#images.get(dto.texture, 'exact');
    if (img === MISSING) return undefined;
    // `Exact` already removed magenta; any other key is applied once per atlas
    const key = dto.color_key >>> 0;
    const out = key === 0 || key === MAGENTA_KEY ? img : applyColorKey(img, key);
    this.#keyed.set(dto.texture, out);
    return out;
  }
}
