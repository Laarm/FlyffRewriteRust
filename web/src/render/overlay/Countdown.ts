// The PvP countdown of the world overlay (Plan/41 section 19.4, WP-REND-25; `CMover::RenderPVPCount`, `MoverRender.cpp:2325-2368`): the sprite
// of the number (3, 2, 1, then the word "fight") shows for 1.5 s in the middle of the view, 50 px above the centre. For 250 ms it stands
// at scale 1 and alpha 180, then it grows to scale 2 while the alpha fades by 10 per 100 ms. The texture is `SFX/CountFight.inc`: the sprites 0-9
// are the digits (64 x 64), the sprite 10 the word (330 x 64). Count 0 is the word.

import type { SpriteItem } from './types';

export const COUNTDOWN_ATLAS = 'sfx/countfight.inc';
/** How long a `DuelCount` stays on the screen (ms). */
export const COUNTDOWN_MS = 1500;
/** The sprite of the word "fight". */
export const COUNTDOWN_FIGHT_SPRITE = 10;

export interface CountdownLook {
  readonly sprite: number;
  /** `fScal`. */
  readonly scale: number;
  /** `nAlpha`, 0-255. */
  readonly alpha: number;
}

/** What the countdown looks like `elapsedMs` after the message `n`; `null` when it is not on the screen. */
export function countdownLook(elapsedMs: number, n: number): CountdownLook | null {
  if (!(elapsedMs >= 0 && elapsedMs <= COUNTDOWN_MS)) return null;
  const sprite = n === 0 ? COUNTDOWN_FIGHT_SPRITE : n;
  if (sprite < 0 || sprite > COUNTDOWN_FIGHT_SPRITE) return null;
  let scale = elapsedMs / 10;
  let alpha = 180;
  if (scale < 25) {
    scale = 1;
  } else {
    scale = scale / 10 - 1.5;
    alpha = Math.trunc(alpha - scale * 10);
  }
  if (scale > 2) scale = 2;
  return { sprite, scale, alpha: Math.max(0, alpha) };
}

/** The top-left corner of the sprite: centred, 50 px above the centre of the view, the x made even (`MakeEven`). */
export function countdownOrigin(
  view: { readonly width: number; readonly height: number },
  size: { readonly w: number; readonly h: number },
  scale: number,
): { x: number; y: number } {
  const x = Math.trunc(view.width / 2 - (size.w * scale) / 2);
  const y = Math.trunc(view.height / 2 - (size.h * scale) / 2 - 50);
  return { x: x - (x % 2), y };
}

/** The sprite item of the countdown `n` at `elapsedMs`, for a sprite of `size`; `null` when nothing shows. */
export function layoutCountdown(
  n: number,
  elapsedMs: number,
  view: { readonly width: number; readonly height: number },
  size: { readonly w: number; readonly h: number },
): SpriteItem | null {
  const look = countdownLook(elapsedMs, n);
  if (look === null) return null;
  const o = countdownOrigin(view, size, look.scale);
  return {
    kind: 'sprite',
    atlas: COUNTDOWN_ATLAS,
    index: look.sprite,
    x: o.x,
    y: o.y,
    alpha: look.alpha,
    scale: look.scale,
  };
}
