// The pixels of the scroll bar (`CWndScrollBar::PaintFrame` + `OnDraw` + the two arrow buttons; Plan/42 §8.9):
//  * the track `ButtVScrBar.bmp` (16 x 4) stretched vertically from y = 16 to `4 * floor((h - 32) / 4)` px;
//  * the arrows `ButtVScrUp.tga` / `ButtVScrDown.tga` (64 x 16 = four 16 x 16 frames, hover 0 / normal 1 / pushed 2)
//    clipped to the 15 x 15 button;
//  * the thumb `ButtVScrPUp.bmp` + `ButtVScrPBar.bmp` stretched + `ButtVScrPDown.bmp` (16 x 4 each) when the range
//    exceeds the page.

import { useSyncExternalStore, type CSSProperties } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import type { ScrollBar } from './ScrollBar';
import { SCROLLBAR_WIDTH, thumbGeometry } from './scrollMath';
import { frameBackground, type Sprite, type SpriteState } from './sprites';
import { useSprite } from './useSprite';

const ready = (s: SpriteState): s is Sprite => typeof s === 'object';

/** The CSS of a sprite stretched to a box. */
function stretched(s: Sprite, w: number, h: number): CSSProperties {
  return {
    backgroundImage: `url("${s.url}")`,
    backgroundSize: `${String(w)}px ${String(h)}px`,
    backgroundRepeat: 'no-repeat',
  };
}

export function ScrollBarView({ control }: { control: ScrollBar }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const env = control.env;
  const up = useSprite(env, 'ButtVScrUp.tga');
  const down = useSprite(env, 'ButtVScrDown.tga');
  const track = useSprite(env, 'ButtVScrBar.bmp');
  const pUp = useSprite(env, 'ButtVScrPUp.bmp');
  const pBar = useSprite(env, 'ButtVScrPBar.bmp');
  const pDown = useSprite(env, 'ButtVScrPDown.bmp');
  const w = rectWidth(s.rect);
  const h = rectHeight(s.rect);
  const a = SCROLLBAR_WIDTH;
  const bottom = Math.floor(h / 4) * 4;
  const frameOf = (part: 'up' | 'down'): number =>
    s.pressed === part ? 2 : s.hover === part ? 0 : 1;
  const trackH = 4 * Math.floor((h - 32) / 4);
  const thumb = thumbGeometry(h, s.min, s.max, s.page, s.pos, a);
  const midH = thumb ? 4 * (Math.floor(thumb.height / 4) - 1) : 0;
  return (
    <div className="sb" data-testid="scrollbar" style={{ width: w, height: h }}>
      {ready(track) && trackH > 0 && (
        <div
          className="sb-track"
          style={{ left: 0, top: 16, width: 16, height: trackH, ...stretched(track, 16, trackH) }}
        />
      )}
      {ready(up) && (
        <div
          className="sb-arrow sb-up"
          data-frame={frameOf('up')}
          style={{ left: 0, top: 0, width: a, height: a, ...frameBackground(up, 4, frameOf('up')) }}
        />
      )}
      {ready(down) && (
        <div
          className="sb-arrow sb-down"
          data-frame={frameOf('down')}
          style={{
            left: 0,
            top: bottom - a,
            width: a,
            height: a,
            ...frameBackground(down, 4, frameOf('down')),
          }}
        />
      )}
      {thumb && (
        <div
          className="sb-thumb"
          data-top={thumb.top}
          data-height={thumb.height}
          style={{ left: 0, top: thumb.top, width: 16, height: thumb.height }}
        >
          {ready(pUp) && (
            <div
              className="sb-cap"
              style={{ left: 0, top: 0, width: 16, height: 4, ...stretched(pUp, 16, 4) }}
            />
          )}
          {ready(pBar) && midH > 0 && (
            <div
              className="sb-cap"
              style={{ left: 0, top: 4, width: 16, height: midH, ...stretched(pBar, 16, midH) }}
            />
          )}
          {ready(pDown) && (
            <div
              className="sb-cap"
              style={{
                left: 0,
                top: thumb.height - 4,
                width: 16,
                height: 4,
                ...stretched(pDown, 16, 4),
              }}
            />
          )}
        </div>
      )}
    </div>
  );
}
