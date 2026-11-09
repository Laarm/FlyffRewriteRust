// The enchant glow strip (Plan/42 §6.3): `Icon/Icon_ImgIncAni_00..10.tga` (11 frames, 32 x 32) are loaded once, composed
// into one 352 x 32 strip and handed to CSS as `--flyff-enchant-strip: url(blob:...)`; the `.flyff-enchant` class
// then steps through it (`steps(11)` over 11 x 16.667 ms) without any per-frame JavaScript.

import { MISSING, type UiImageCache } from '../skin/images';
import type { PixelImage } from '../skin/pixels';
import { composeEnchantStrip, ENCHANT_FRAMES } from './anim';

/** The VFS path of enchant frame `i`. */
export function enchantFramePath(i: number): string {
  return `icon/icon_imgincani_${String(i).padStart(2, '0')}.tga`;
}

/** Loads the 11 frames (`exact` key) and composes the strip; a missing frame stays transparent (KQ-30). */
export async function loadEnchantStrip(images: UiImageCache): Promise<PixelImage> {
  const frames = await Promise.all(
    Array.from({ length: ENCHANT_FRAMES }, (_, i) => images.get(enchantFramePath(i), 'exact')),
  );
  return composeEnchantStrip(frames.map((f) => (f === MISSING ? undefined : f)));
}

/** A PNG object URL of an image (browser only: `OffscreenCanvas` or a 2-D canvas); `null` without canvas support. */
export async function imageToObjectUrl(img: PixelImage): Promise<string | null> {
  const data = new ImageData(img.data as Uint8ClampedArray<ArrayBuffer>, img.width, img.height);
  let blob: Blob | null;
  if (typeof OffscreenCanvas === 'function') {
    const c = new OffscreenCanvas(img.width, img.height);
    const ctx = c.getContext('2d');
    if (!ctx) return null;
    ctx.putImageData(data, 0, 0);
    blob = await c.convertToBlob({ type: 'image/png' });
  } else {
    const c = document.createElement('canvas');
    c.width = img.width;
    c.height = img.height;
    const ctx = c.getContext('2d');
    if (!ctx) return null;
    ctx.putImageData(data, 0, 0);
    blob = await new Promise<Blob | null>((resolve) => {
      c.toBlob(resolve, 'image/png');
    });
  }
  return blob ? URL.createObjectURL(blob) : null;
}

/** Composes the strip and publishes it as the `--flyff-enchant-strip` custom property of `root`. */
export async function installEnchantStrip(
  images: UiImageCache,
  root: HTMLElement = document.documentElement,
): Promise<boolean> {
  const url = await imageToObjectUrl(await loadEnchantStrip(images));
  if (!url) return false;
  root.style.setProperty('--flyff-enchant-strip', `url("${url}")`);
  return true;
}
