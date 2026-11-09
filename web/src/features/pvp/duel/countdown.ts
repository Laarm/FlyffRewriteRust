// The duel countdown (`CMover::RenderPVPCount`, `src:_Common/MoverRender.cpp:2325-2368`, `DuelCount` ref/09a §5.8; WP-CFEAT-36): the sprite of the
// number (3, 2, 1, then "fight") shows for 1.5 s in the middle of the view, 50 px above the centre. Its scale and alpha follow the time since
// the message: 250 ms at scale 1 and alpha 180, then the sprite grows to scale 2 (reached after 100 ms more) while the alpha fades by 10
// per 100 ms. The texture is `SFX/CountFight.inc`: sprites 0-9 are the digits (64 x 64), sprite 10 is the "fight" word (330 x 64). The DOM
// overlay (`Countdown.tsx`) writes the number in text with the look of the sprites; the sprite index is the number it shows.

/** How long a `DuelCount` stays on the screen (ms). */
export const COUNTDOWN_MS = 1500;
/** The sprite of the "fight" word (count 0 becomes 10 in the original). */
export const COUNTDOWN_FIGHT_SPRITE = 10;

export interface CountdownLook {
  /** The index into the atlas. */
  readonly sprite: number;
  /** `fScal`. */
  readonly scale: number;
  /** `nAlpha` (0-255). */
  readonly alpha: number;
}

/**
 * What the countdown looks like `elapsedMs` after the message `n`; `null` when it is not on the screen (before the message or after 1.5 s,
 * or for a number the atlas has no sprite for).
 */
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
  // `if( 1.5f < fScal ) { if( count == 0 ) fScal = 1.5f; else if( 2.0f < fScal ) fScal = 2.0f; }`: the count is never 0 here (it became 10)
  if (scale > 2) scale = 2;
  return { sprite, scale, alpha };
}

/** The top-left corner of the sprite: centred, 50 px above the centre of the view, the x made even (`MakeEven`). */
export function countdownOrigin(
  viewport: { readonly width: number; readonly height: number },
  size: { readonly w: number; readonly h: number },
  scale: number,
): { x: number; y: number } {
  const x = Math.trunc(viewport.width / 2 - (size.w * scale) / 2);
  const y = Math.trunc(viewport.height / 2 - (size.h * scale) / 2 - 50);
  return { x: x - (x % 2), y };
}
