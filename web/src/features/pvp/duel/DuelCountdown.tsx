// The DOM face of the duel countdown (Plan/45 §7.11; WP-CFEAT-36): the number of `DuelCount` over the world view, painted under the windows,
// placed and faded by `countdown.ts` (`RenderPVPCount`). The original draws sprites of `SFX/CountFight.dds` (italic numerals with an orange
// gradient and a purple outline, and the word "FIGHT !!"). The real sprites are drawn by the canvas overlay of the renderer (`ctx.render.pvpCountdown`,
// the UI image cache decodes the DDS of `sfx/countfight.inc` through the decode pool); this text is what shows until they are resident and
// when there is no overlay: an italic heavy face, the orange gradient of the sprites and their purple outline. It hides (`canvasDrawn`) as soon as
// the overlay draws the sprite. `pvpStore.count` is the contract of both.

import { useEffect, useState, useSyncExternalStore, type CSSProperties } from 'react';
import type { PvpStore } from '../store';
import { COUNTDOWN_FIGHT_SPRITE, countdownLook, countdownOrigin } from './countdown';

/** The logical size of the sprites of `CountFight.inc`: the digits 64 x 64, the word 330 x 64. */
const DIGIT_SIZE = { w: 64, h: 64 } as const;
const WORD_SIZE = { w: 330, h: 64 } as const;
const FAMILY = '"Flyff DejaVu Sans", Verdana, sans-serif';

export interface DuelCountdownProps {
  readonly store: PvpStore;
  readonly now: () => number;
  readonly viewport: { width: number; height: number };
  readonly scale: number;
  /** The canvas overlay draws the sprite of the countdown at this moment: the text stands down (it is the fallback until the sprites are resident). */
  readonly canvasDrawn?: () => boolean;
}

export function DuelCountdown({ store, now, viewport, scale, canvasDrawn }: DuelCountdownProps) {
  const count = useSyncExternalStore(store.subscribe, () => store.getState().count);
  const [, frame] = useState(0);
  // the animation runs while a number is on the screen
  useEffect(() => {
    if (count === null) return undefined;
    const id = setInterval(() => {
      frame((n) => n + 1);
    }, 33);
    return () => {
      clearInterval(id);
    };
  }, [count]);
  const look = count === null ? null : countdownLook(now() - count.at, count.n);
  if (!look || canvasDrawn?.() === true) return null;
  const word = look.sprite === COUNTDOWN_FIGHT_SPRITE;
  const size = word ? WORD_SIZE : DIGIT_SIZE;
  const at = countdownOrigin(viewport, size, look.scale);
  const container: CSSProperties = {
    position: 'fixed',
    left: 0,
    top: 0,
    width: viewport.width,
    height: viewport.height,
    transform: scale === 1 ? undefined : `scale(${String(scale)})`,
    transformOrigin: '0 0',
    pointerEvents: 'none',
    zIndex: 0,
  };
  const px = (word ? 56 : 66) * look.scale;
  const text = word ? 'FIGHT !!' : String(look.sprite);
  // the group carries the alpha, so that the layers are not darkened twice where they overlap
  const box: CSSProperties = {
    position: 'absolute',
    left: at.x,
    top: at.y,
    width: size.w * look.scale,
    height: size.h * look.scale,
    opacity: look.alpha / 255,
  };
  const glyph: CSSProperties = {
    position: 'absolute',
    inset: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontFamily: FAMILY,
    fontStyle: 'italic',
    fontWeight: 900,
    fontSize: px,
    lineHeight: 1,
    letterSpacing: word ? px * 0.04 : 0,
    whiteSpace: 'pre',
  };
  // the purple outline of the sprites is a layer of its own below the gradient (a stroke or a text shadow on the clipped layer would paint
  // over its background); the stroke is centred on the glyph edge and the fill layer covers its inner half
  const outline: CSSProperties = {
    ...glyph,
    color: '#8a1a5c',
    WebkitTextStroke: `${String(Math.max(4, 8 * look.scale))}px #8a1a5c`,
  };
  const fill: CSSProperties = {
    ...glyph,
    backgroundImage: 'linear-gradient(180deg, #fff06a 0%, #ff9d00 50%, #ff4a00 100%)',
    WebkitBackgroundClip: 'text',
    backgroundClip: 'text',
    color: 'transparent',
  };
  return (
    <div style={container} data-testid="duel-countdown-layer">
      <div style={box}>
        <div style={outline} aria-hidden="true">
          {text}
        </div>
        <div style={fill} data-testid="duel-countdown" data-n={count?.n}>
          {text}
        </div>
      </div>
    </div>
  );
}
