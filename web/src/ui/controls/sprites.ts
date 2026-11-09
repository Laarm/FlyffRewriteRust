// Sprite strips of the textured controls (Plan/42 §8.5, §8.9; ref/22 §6.6): a button, a scroll-bar arrow, a tab tile
// or a tree icon is one image the browser shows as a CSS background. The URL comes from the host (the `Exact` key
// mode, magenta -> transparent), the natural size from the decoded image (the cache keeps it, 64 MB LRU), so the view
// can cut one frame out of a strip: `frames` equal frames side by side (4 for push buttons, 6 for check/radio).

import { MISSING, themePath } from '../skin/images';
import type { WndEnv } from '../wnd/env';

/** One loaded image: where it is and how big it is. */
export interface Sprite {
  url: string;
  width: number;
  height: number;
}

/** The state of a sprite load: `loading` renders nothing (no flash of the untextured look), `missing` falls back. */
export type SpriteState = Sprite | 'loading' | 'missing';

/**
 * Loads a theme image (`ButtOk.tga`, `Icon/...`) in the `Exact` mode. Resolves to `null` for a file that does not
 * exist (KQ-30: nothing is drawn for it, the control falls back to its untextured look when it has one).
 */
export async function loadSprite(env: WndEnv, path: string): Promise<Sprite | null> {
  const full = themePath(path);
  const img = await env.images.get(full, 'exact');
  if (img === MISSING) return null;
  const url = env.images.url(full, 'exact');
  return url === null ? null : { url, width: img.width, height: img.height };
}

/** Whether the image is already decoded (no flash for images of earlier windows): its `Sprite`, else `undefined`. */
export function peekSprite(env: WndEnv, path: string): Sprite | 'missing' | undefined {
  const full = themePath(path);
  const img = env.images.peek(full, 'exact');
  if (img === undefined) return undefined;
  if (img === MISSING) return 'missing';
  const url = env.images.url(full, 'exact');
  return url === null ? 'missing' : { url, width: img.width, height: img.height };
}

/** The CSS of frame `index` of a strip of `frames` equal frames, shown at its natural size at (0, 0). */
export function frameBackground(
  sprite: Sprite,
  frames: number,
  index: number,
): {
  backgroundImage: string;
  backgroundSize: string;
  backgroundPosition: string;
  backgroundRepeat: string;
} {
  const fw = Math.floor(sprite.width / frames);
  return {
    backgroundImage: `url("${sprite.url}")`,
    backgroundSize: `${String(sprite.width)}px ${String(sprite.height)}px`,
    backgroundPosition: `${String(-index * fw)}px 0`,
    backgroundRepeat: 'no-repeat',
  };
}

/** The width of one frame of a strip (`m_pTexture->m_size.cx / frames`). */
export function frameWidth(sprite: Sprite, frames: number): number {
  return Math.floor(sprite.width / frames);
}
