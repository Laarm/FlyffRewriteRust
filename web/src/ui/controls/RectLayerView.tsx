// The pixels of a rectangle layer: one absolutely positioned `div` per entry (fill, outline, glow as a box shadow).

import { useSyncExternalStore } from 'react';
import type { RectEntry, RectLayer } from './RectLayer';

function Entry({ entry }: { entry: RectEntry }) {
  const style: React.CSSProperties = {
    position: 'absolute',
    left: entry.x,
    top: entry.y,
    width: entry.w,
    height: entry.h,
    boxSizing: 'border-box',
    pointerEvents: 'none',
  };
  if (entry.fill) style.background = entry.fill;
  if (entry.outline) style.border = `${String(entry.outline.width)}px solid ${entry.outline.color}`;
  if (entry.glow)
    style.boxShadow = `0 0 ${String(entry.glow.size)}px ${String(entry.glow.size / 2)}px ${entry.glow.color}`;
  return <div className="rect-entry" data-testid="rect-entry" data-key={entry.key} style={style} />;
}

export function RectLayerView({ control }: { control: RectLayer }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  return (
    <div data-testid="rect-layer" style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
      {s.entries.map((e) => (
        <Entry key={e.key} entry={e} />
      ))}
    </div>
  );
}
