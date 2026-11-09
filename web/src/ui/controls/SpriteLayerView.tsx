// The pixels of a sprite layer: one absolutely positioned `div` per entry with the image as a CSS background (`useSprite` loads each
// file once and shares it). A cut-out shows the rectangle at `src` at natural scale; a whole image is scaled to the entry's size.

import { useSyncExternalStore } from 'react';
import type { SpriteEntry, SpriteLayer } from './SpriteLayer';
import { useSprite } from './useSprite';

function Entry({ layer, entry }: { layer: SpriteLayer; entry: SpriteEntry }) {
  const sprite = useSprite(layer.env, entry.path);
  const url = typeof sprite === 'object' ? sprite.url : null;
  const style: React.CSSProperties = {
    position: 'absolute',
    left: entry.x,
    top: entry.y,
    width: entry.w,
    height: entry.h,
    backgroundRepeat: 'no-repeat',
    pointerEvents: 'none',
  };
  if (entry.alpha !== undefined && entry.alpha !== 1) style.opacity = entry.alpha;
  if (typeof sprite === 'object' && url) {
    style.backgroundImage = `url("${url}")`;
    if (entry.src) {
      const k = entry.scale ?? 1;
      style.backgroundSize = `${String(sprite.width * k)}px ${String(sprite.height * k)}px`;
      style.backgroundPosition = `${String(-entry.src.x * k)}px ${String(-entry.src.y * k)}px`;
    } else {
      style.backgroundSize = `${String(entry.w)}px ${String(entry.h)}px`;
    }
  }
  return (
    <div
      className="sprite"
      data-testid="sprite"
      data-key={entry.key}
      data-path={entry.path ?? undefined}
      style={style}
    />
  );
}

export function SpriteLayerView({ control }: { control: SpriteLayer }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  return (
    <div
      data-testid="sprite-layer"
      style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}
    >
      {s.entries.map((e) => (
        <Entry key={e.key} layer={control} entry={e} />
      ))}
    </div>
  );
}
