// The pixels of the paper doll: one icon per layer, the stack count in the title font (`__NEW_FONT_COLOR`) or the legacy blue pair.

import { useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { iconImageStyle } from '../../icons/IconCell';
import { rectWidth } from '../../layout/geometry';
import { FONT, fontStyle } from '../../text/fonts';
import type { PaperDoll, PaperLayer } from './PaperDoll';

function Layer({
  control,
  layer,
  newFontColor,
  width,
}: {
  control: PaperDoll;
  layer: PaperLayer;
  newFontColor: boolean;
  width: number;
}) {
  const sprite = useSprite(control.env, layer.icon);
  const url = typeof sprite === 'object' ? sprite.url : null;
  const count = layer.count;
  const font = newFontColor ? FONT.title : FONT.text;
  return (
    <>
      <div
        className="paper-icon"
        data-testid="paper-icon"
        data-key={layer.key}
        data-tint={layer.tint ? 'red' : undefined}
        style={iconImageStyle({
          url,
          size: layer.size,
          ...(layer.height === undefined ? {} : { height: layer.height }),
          left: layer.x,
          top: layer.y,
          alpha: layer.alpha,
          tint: layer.tint,
        })}
      />
      {count && (
        <>
          {!newFontColor && (
            // the legacy pair: blue at (x, y), light blue one pixel up and left
            <span
              className="paper-count"
              data-testid="paper-count-shadow"
              style={{
                ...fontStyle(font),
                right: width - count.right,
                top: count.top,
                color: '#0000FF',
              }}
            >
              {count.text}
            </span>
          )}
          <span
            className="paper-count"
            data-testid="paper-count"
            data-key={layer.key}
            style={{
              ...fontStyle(font),
              right: width - count.right + (newFontColor ? 0 : 1),
              top: count.top - (newFontColor ? 0 : 1),
              color: newFontColor ? '#EAEBDD' : '#B0B0F0',
            }}
          >
            {count.text}
          </span>
        </>
      )}
    </>
  );
}

export function PaperDollView({ control }: { control: PaperDoll }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const width = rectWidth(s.rect);
  return (
    <div className="paper" data-testid="paper-doll">
      {s.layers.map((l) => (
        <Layer
          key={l.key}
          control={control}
          layer={l}
          newFontColor={s.newFontColor}
          width={width}
        />
      ))}
    </div>
  );
}
