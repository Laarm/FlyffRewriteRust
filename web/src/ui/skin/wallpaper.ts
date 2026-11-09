// The title-screen wallpaper (Plan/42 §7.7; `CTheme::RenderWallpaper`, ITheme.cpp:780-830): on the login and
// character states the root paints `Theme/Screen.bmp` (wallpaper type 1, STRETCH) on the clear colour `0xFF000000`.
// This module is the pure part (the box rule) plus a painter for a 2-D canvas; the title screens of WP-UI-09/11 mount it.

import type { Rect } from '../layout/geometry';
import { rect } from '../layout/geometry';

/** The wallpaper file of `Default.thm` (`m_texWallPaper`). */
export const WALLPAPER_FILE = 'theme/screen.bmp';

/**
 * The box the image is stretched into: centred, `min(viewport.w, round(viewport.h * 4 / 3))` wide and the full
 * height; black outside. It generalises the original's wide-mode table (1280x720/768/800 -> 960 px, 1360x768 ->
 * 1024, 1440x900 -> 1200, 1680x1050 -> 1400): 720 -> 960, 768 -> 1024, 900 -> 1200, 1050 -> 1400.
 */
export function wallpaperBox(viewportW: number, viewportH: number): Rect {
  const w = Math.min(viewportW, Math.round((viewportH * 4) / 3));
  const l = Math.trunc((viewportW - w) / 2);
  return rect(l, 0, l + w, viewportH);
}

/** Paints the wallpaper: black clear colour, the image stretched into {@link wallpaperBox}. */
export function paintWallpaper(
  ctx: CanvasRenderingContext2D,
  image: CanvasImageSource,
  viewportW: number,
  viewportH: number,
): void {
  const b = wallpaperBox(viewportW, viewportH);
  ctx.fillStyle = '#000';
  ctx.fillRect(0, 0, viewportW, viewportH);
  ctx.imageSmoothingEnabled = true;
  ctx.drawImage(image, b.l, b.t, b.r - b.l, b.b - b.t);
}
