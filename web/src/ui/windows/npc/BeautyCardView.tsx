// The pixels of a beauty card: a framed box, the colour swatch and the label.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { FONT, fontStyle, lineHeight } from '../../text/fonts';
import type { BeautyCard } from './BeautyCard';

const hex = (n: number): string => `#${n.toString(16).padStart(6, '0')}`;

export function BeautyCardView({ control }: { control: BeautyCard }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const width = rectWidth(w);
  const height = rectHeight(w);
  if (s.overlay) {
    // the 3-D preview is behind: the selected card gets the translucent yellow fill of the original (`0x60ffff00`), above the canvas
    // (the custom canvases are mounted after the controls, so the card needs a z-index of its own)
    return (
      <div
        data-testid="beauty-card"
        data-selected={s.selected}
        data-label={s.label}
        data-overlay="true"
        style={{
          position: 'relative',
          zIndex: 1,
          width,
          height,
          background: s.selected ? 'rgba(255, 255, 0, 0.376)' : 'transparent',
          cursor: s.pickable ? 'pointer' : 'default',
        }}
      />
    );
  }
  return (
    <div
      data-testid="beauty-card"
      data-selected={s.selected}
      data-label={s.label}
      style={{
        position: 'relative',
        width,
        height,
        boxSizing: 'border-box',
        border: `2px solid ${s.selected ? '#ff8000' : s.hover && s.pickable ? '#ffc080' : '#8a8a8a'}`,
        background: 'rgba(255, 255, 255, 0.25)',
        cursor: s.pickable ? 'pointer' : 'default',
      }}
    >
      {s.swatch !== null && (
        <div
          data-testid="beauty-swatch"
          style={{
            position: 'absolute',
            left: 6,
            top: 6,
            right: 6,
            bottom: lineHeight(FONT.text) + 8,
            background: hex(s.swatch),
            border: '1px solid #404040',
          }}
        />
      )}
      <div
        data-testid="beauty-label"
        style={{
          position: 'absolute',
          left: 0,
          right: 0,
          bottom: 3,
          textAlign: 'center',
          color: '#000',
          whiteSpace: 'pre',
          ...fontStyle(FONT.text),
        }}
      >
        {s.label}
      </div>
    </div>
  );
}
