// The DOM face of the HUD messages (Plan/45 §7.11, §0.2): the captions at the top of the world view and the centre client
// messages above the lower third, painted under the windows and over the world canvas. Positions are in the virtual
// pixels of 42's UI (the container is scaled like `UiRoot`), fonts follow the original: the world font (12 px, 2 px outline in
// the font's own colour) for the centre lines, the caption fonts (viewport width / 20 and / 50, 2 px outline `(32, 96, 208)`)
// for the captions.

import { useSyncExternalStore, type CSSProperties, type ReactNode } from 'react';
import { argbCss, outlineOffsets } from '../../render/overlay/GlyphAtlas';
import { parseMarkup } from '../../ui/text/markup';
import { Captions } from './captions';
import type { HudState } from './hud';

const FAMILY = '"Flyff DejaVu Sans", Verdana, sans-serif';
/** The line pitch of the centre messages (`size.cy + m_nLineSpace`: 19 - 3). */
const CENTRE_PITCH = 16;
const CENTRE_CELL = 19;

function outline(px: number, rgb: string): string {
  return outlineOffsets(px)
    .map(([dx, dy]) => `${String(dx)}px ${String(dy)}px 0 ${rgb}`)
    .join(',');
}

/**
 * The text of a centre line with its markup applied: `CClientMsg::AddMessage` parses the text (`SetParsingString`, so `#b` .. `#nb` of
 * a quest kill line is bold, not printed) and `Render` then overwrites the colour of every character with the line's own, so only the
 * style bits of the codes (bold, underline, strike-through) survive; the colour codes are consumed.
 */
function centreRuns(text: string): ReactNode {
  const runs = parseMarkup(text, 0xffffffff);
  if (runs.every((r) => !r.bold && !r.underline && !r.strike))
    return runs.map((r) => r.text).join('');
  return runs.map((r, i) => {
    const decoration = [r.underline ? 'underline' : '', r.strike ? 'line-through' : '']
      .filter((d) => d !== '')
      .join(' ');
    const style: CSSProperties = {
      fontWeight: r.bold ? 'bold' : undefined,
      textDecoration: decoration === '' ? undefined : decoration,
    };
    return (
      <span key={i} style={style}>
        {r.text}
      </span>
    );
  });
}

export interface HudOverlayProps {
  hud: HudState;
  /** The virtual viewport of the UI. */
  viewport: { width: number; height: number };
  /** The UI scale (`UiScaler`). */
  scale: number;
}

export function HudOverlay({ hud, viewport, scale }: HudOverlayProps) {
  useSyncExternalStore(hud.subscribe, hud.getVersion);
  const { width, height } = viewport;
  const lines = hud.centre.lines;
  const baseY = Math.trunc((520 * height) / 768);
  const count = lines.length;
  // each caption sits `size.h` below the previous one, starting at 184 / 768 of the height
  const capTops: number[] = [];
  let acc = Math.trunc((184 * height) / 768);
  for (const c of hud.captions.list) {
    capTops.push(acc);
    acc += c.size.h;
  }
  const container: CSSProperties = {
    position: 'fixed',
    left: 0,
    top: 0,
    width,
    height,
    transform: scale === 1 ? undefined : `scale(${String(scale)})`,
    transformOrigin: '0 0',
    pointerEvents: 'none',
    zIndex: 0,
    fontFamily: FAMILY,
  };
  return (
    <div style={container} data-testid="hud-overlay">
      {hud.captions.list.slice(0, hud.captions.count).map((c, i) => {
        const px = hud.captionPx(c.small);
        const f = Captions.drawScale(c);
        const a = Captions.drawAlpha(c);
        const top = capTops[i] ?? 0;
        if (c.alpha <= 0 || a <= 0) return null;
        const style: CSSProperties = {
          position: 'absolute',
          left: Math.trunc(width / 2 - (c.size.w / 2) * f),
          top,
          fontSize: px,
          lineHeight: `${String(c.size.h)}px`,
          whiteSpace: 'pre',
          transformOrigin: '0 0',
          transform: `scale(${String(f)})`,
          color: `rgba(250,250,255,${(Math.min(255, a) / 255).toFixed(3)})`,
          textShadow: outline(2, `rgba(32,96,208,${(Math.min(255, a) / 255).toFixed(3)})`),
        };
        return (
          <div key={`${String(i)}:${c.text}`} style={style} data-testid="hud-caption">
            {c.text}
          </div>
        );
      })}
      {lines.map((l, i) => {
        // the newest line is the lowest; the block is centred on `baseY`
        const fromBottom = count - 1 - i;
        const top = Math.trunc(
          baseY + (count * CENTRE_PITCH) / 2 - CENTRE_PITCH - fromBottom * CENTRE_PITCH,
        );
        const alpha = Math.max(0, l.alpha);
        const style: CSSProperties = {
          position: 'absolute',
          left: 0,
          width,
          top,
          textAlign: 'center',
          fontSize: 12,
          lineHeight: `${String(CENTRE_CELL)}px`,
          whiteSpace: 'pre',
          color: argbCss(((alpha << 24) | (l.color & 0xffffff)) >>> 0),
          textShadow: outline(2, `rgba(48,48,48,${(alpha / 255).toFixed(3)})`),
        };
        return (
          <div key={`${String(i)}:${l.text}`} style={style} data-testid="hud-centre-line">
            {centreRuns(l.text)}
          </div>
        );
      })}
    </div>
  );
}
