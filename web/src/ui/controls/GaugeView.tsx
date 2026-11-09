// The pixels of a gauge: a three-slice bar for the empty part and one for the fill (Plan/42 §8.10), as three DOM boxes
// per layer so the browser stretches the middle slice.

import { useSyncExternalStore, type CSSProperties } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import type { Gauge } from './Gauge';
import { gaugeFillWidth, gaugeSlices } from './Gauge';
import type { Sprite } from './sprites';
import { useSprite } from './useSprite';

/** The RGB part of a vertex colour as a CSS colour, `null` for white (no modulation). */
export function gaugeTint(color: number): string | null {
  const rgb = color & 0xffffff;
  if (rgb === 0xffffff) return null;
  return `rgb(${String((rgb >>> 16) & 0xff)}, ${String((rgb >>> 8) & 0xff)}, ${String(rgb & 0xff)})`;
}

/**
 * `texture * vertex colour`: a flat colour multiplied into the slice, clipped by the slice's own alpha (the mask repeats the
 * background geometry), so the transparent colour-key pixels stay transparent.
 */
function tinted(
  style: CSSProperties,
  url: string,
  tint: string | null,
  size: string,
  pos: string,
): CSSProperties {
  if (tint === null) return style;
  return {
    ...style,
    backgroundColor: tint,
    backgroundBlendMode: 'multiply',
    maskImage: `url("${url}")`,
    maskSize: size,
    maskPosition: pos,
    maskRepeat: 'no-repeat',
  };
}

/** One three-slice bar of a gauge (the empty part or the fill); also drawn by the target frame of 45 (WP-CFEAT-09). */
export function GaugeBar({
  sprite,
  width,
  alpha,
  tint = null,
}: {
  sprite: Sprite;
  width: number;
  alpha: number;
  tint?: string | null;
}) {
  const sliceW = Math.floor(sprite.width / 3);
  const n = gaugeSlices(width, sliceW);
  if (n === 0) return null;
  const h = sprite.height;
  const base: CSSProperties = {
    position: 'absolute',
    top: 0,
    height: h,
    backgroundImage: `url("${sprite.url}")`,
    backgroundRepeat: 'no-repeat',
    opacity: alpha,
  };
  const capSize = `${String(sprite.width)}px ${String(h)}px`;
  const mid = (n - 2) * sliceW;
  return (
    <>
      <div
        className="gau-cap"
        style={tinted(
          {
            ...base,
            left: 0,
            width: sliceW,
            backgroundSize: capSize,
            backgroundPosition: '0 0',
          },
          sprite.url,
          tint,
          capSize,
          '0 0',
        )}
      />
      {mid > 0 && (
        <div
          className="gau-mid"
          style={tinted(
            {
              ...base,
              left: sliceW,
              width: mid,
              backgroundSize: `${String(mid * 3)}px ${String(h)}px`,
              backgroundPosition: `${String(-mid)}px 0`,
            },
            sprite.url,
            tint,
            `${String(mid * 3)}px ${String(h)}px`,
            `${String(-mid)}px 0`,
          )}
        />
      )}
      <div
        className="gau-cap"
        style={tinted(
          {
            ...base,
            left: (n - 1) * sliceW,
            width: sliceW,
            backgroundSize: capSize,
            backgroundPosition: `${String(-2 * sliceW)}px 0`,
          },
          sprite.url,
          tint,
          capSize,
          `${String(-2 * sliceW)}px 0`,
        )}
      />
    </>
  );
}

export function GaugeView({ control }: { control: Gauge }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const bg = useSprite(control.env, s.background);
  const fill = useSprite(control.env, s.fill);
  const w = rectWidth(s.rect);
  const h = rectHeight(s.rect);
  const alpha = (s.color >>> 24) / 255;
  return (
    <div
      className="gau"
      data-testid="gauge"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={s.max}
      aria-valuenow={Math.max(0, Math.min(s.value, s.max))}
      style={{ width: w, height: h }}
    >
      {typeof bg === 'object' && <GaugeBar sprite={bg} width={w} alpha={1} />}
      {typeof fill === 'object' && (
        <GaugeBar
          sprite={fill}
          width={gaugeFillWidth(w, s.value, s.max)}
          alpha={alpha}
          tint={gaugeTint(s.color)}
        />
      )}
    </div>
  );
}
