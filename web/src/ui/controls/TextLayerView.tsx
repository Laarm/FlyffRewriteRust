// The pixels of a text layer: one absolutely positioned line per item in the item's font (the outlined title font draws its
// outline as a text shadow, Plan/42 §8.1).

import { useSyncExternalStore } from 'react';
import { FONT, fontStyle, textMetricsEpoch } from '../text/fonts';
import { cssColor } from '../text/markup';
import type { TextLayer } from './TextLayer';

export function TextLayerView({ control }: { control: TextLayer }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  useSyncExternalStore(textMetricsEpoch.subscribe, textMetricsEpoch.get);
  return (
    <div className="tl" data-testid="text-layer">
      {s.items.map((it, i) => (
        <div
          // the list is rebuilt from a fixed layout: the index is the identity
          key={i}
          className="tl-item"
          style={{
            left: it.x,
            top: it.y,
            color: cssColor(it.color),
            ...fontStyle(it.font === 'title' ? FONT.title : FONT.text),
          }}
        >
          {it.text}
        </div>
      ))}
    </div>
  );
}
