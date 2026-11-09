// The overlay's texture source: an `OverlayAssets` over a loader that hands out decoded RGBA images (the UI's
// `UiAtlases.sprite` and `UiImageCache.get`, wired by the app: `render/` does not import `ui/`). The first request of a
// sprite or image starts the load and answers `null`; the overlay draws nothing for it until it arrived. Each texture is
// copied once into a canvas that `drawImage` samples.

import type { OverlayAssets, OverlayImage } from './types';

/** Decoded straight-alpha RGBA pixels (the shape of `ui/skin/pixels.PixelImage`). */
export interface RgbaImage {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8ClampedArray;
}

/** A sprite rectangle in a decoded texture (the shape of `UiAtlases.sprite`'s result). */
export interface SpriteRect {
  readonly img: RgbaImage;
  readonly sx: number;
  readonly sy: number;
  readonly w: number;
  readonly h: number;
}

/** Where the pixels come from. `image` resolves to `undefined` for a file that does not exist. */
export interface OverlayTextureSource {
  sprite(atlas: string, index: number): Promise<SpriteRect | undefined>;
  image(path: string): Promise<RgbaImage | undefined>;
}

/** Turns RGBA pixels into something `drawImage` accepts (a canvas); `null` where there is no canvas (Node tests). */
export type CanvasMaker = (img: RgbaImage) => CanvasImageSource | null;

/** The browser's canvas maker (`OffscreenCanvas` when available). */
export const browserCanvasMaker: CanvasMaker = (img) => {
  try {
    const canvas =
      typeof OffscreenCanvas !== 'undefined'
        ? new OffscreenCanvas(img.width, img.height)
        : Object.assign(document.createElement('canvas'), { width: img.width, height: img.height });
    const ctx = canvas.getContext('2d') as CanvasRenderingContext2D | null;
    if (!ctx) return null;
    ctx.putImageData(
      new ImageData(img.data as Uint8ClampedArray<ArrayBuffer>, img.width, img.height),
      0,
      0,
    );
    return canvas;
  } catch {
    return null;
  }
};

type Slot = OverlayImage | 'loading' | 'missing';

export interface OverlayTextures extends OverlayAssets {
  /** Resolves when no load is pending (tests, screenshot waits). */
  whenSettled(): Promise<void>;
}

export function createOverlayAssets(
  source: OverlayTextureSource,
  makeCanvas: CanvasMaker = browserCanvasMaker,
): OverlayTextures {
  const sources = new WeakMap<RgbaImage, CanvasImageSource | null>();
  const sprites = new Map<string, Slot>();
  const images = new Map<string, Slot>();
  const pending = new Set<Promise<void>>();

  const canvasOf = (img: RgbaImage): CanvasImageSource | null => {
    if (!sources.has(img)) sources.set(img, makeCanvas(img));
    return sources.get(img) ?? null;
  };
  const track = (p: Promise<void>): void => {
    pending.add(p);
    void p.finally(() => pending.delete(p));
  };

  return {
    sprite(atlas, index) {
      const key = `${atlas}#${String(index)}`;
      const cur = sprites.get(key);
      if (cur !== undefined) return typeof cur === 'string' ? null : cur;
      sprites.set(key, 'loading');
      track(
        source.sprite(atlas, index).then(
          (r) => {
            const canvas = r ? canvasOf(r.img) : null;
            sprites.set(
              key,
              r && canvas ? { source: canvas, sx: r.sx, sy: r.sy, w: r.w, h: r.h } : 'missing',
            );
          },
          () => {
            sprites.set(key, 'missing');
          },
        ),
      );
      return null;
    },
    image(path) {
      const cur = images.get(path);
      if (cur !== undefined) return typeof cur === 'string' ? null : cur;
      images.set(path, 'loading');
      track(
        source.image(path).then(
          (img) => {
            const canvas = img ? canvasOf(img) : null;
            images.set(
              path,
              img && canvas
                ? { source: canvas, sx: 0, sy: 0, w: img.width, h: img.height }
                : 'missing',
            );
          },
          () => {
            images.set(path, 'missing');
          },
        ),
      );
      return null;
    },
    async whenSettled() {
      while (pending.size > 0) await Promise.all([...pending]);
    },
  };
}
