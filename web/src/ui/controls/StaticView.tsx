// The pixels of a static (`CWndStatic::OnDraw`; Plan/42 §8.4): the caption in `FONT.text`, faux bold (drawn at x and
// x + 1: a 1 px text-shadow in the text colour), aligned by `staticTextOrigin`; `WSS_PICTURE` draws the black frame.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { WSS_GROUPBOX, WSS_MONEY, WSS_PICTURE } from '../layout/styles';
import {
  FAUX_BOLD_SHADOW,
  FONT,
  fontStyle,
  lineHeight,
  textMetricsEpoch,
  textWidth,
} from '../text/fonts';
import { formatMoney } from '../text/format';
import { cssColor } from '../text/markup';
import { STATIC_ALIGN_MASK, staticTextOrigin, type Static } from './Static';

export function StaticView({ control }: { control: Static }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  useSyncExternalStore(textMetricsEpoch.subscribe, textMetricsEpoch.get); // re-measure when a font finishes loading
  const w = rectWidth(s.rect);
  const h = rectHeight(s.rect);
  const cell = lineHeight(FONT.text);
  const aligned = (s.style & STATIC_ALIGN_MASK) !== 0;
  // alignment measures the RAW caption even for money statics (KEEP-QUIRK: the original measures `m_strTitle`)
  const textW = aligned ? textWidth(FONT.text, s.text) : 0;
  const { x, y } = staticTextOrigin(s.style, w, h, textW, cell, cell);
  const money = (s.style & WSS_GROUPBOX) === 0 && (s.style & WSS_MONEY) !== 0;
  const picture = (s.style & WSS_PICTURE) !== 0 && (s.style & WSS_GROUPBOX) === 0;
  return (
    <div className="st" data-testid="static" style={{ width: w, height: h }}>
      {picture && control.skinTexture === '' && <div className="st-picture" />}
      <div
        className="st-text"
        style={{
          left: x,
          top: y,
          color: cssColor(control.getTextColor()),
          textShadow: FAUX_BOLD_SHADOW,
          ...fontStyle(FONT.text),
        }}
      >
        {money ? formatMoney(s.text) : s.text}
      </div>
    </div>
  );
}
