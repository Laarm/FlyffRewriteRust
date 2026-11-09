// The pixels of a channel slider: a bevelled track, the filled part in the channel colour and the thumb.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { type ChannelSlider, SLIDER_THUMB, thumbLeft } from './ChannelSlider';

export function ChannelSliderView({ control }: { control: ChannelSlider }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const width = rectWidth(w);
  const height = rectHeight(w);
  const left = thumbLeft(s.value, width);
  return (
    <div
      data-testid="channel-slider"
      data-value={s.value}
      style={{ position: 'relative', width, height }}
    >
      <div
        style={{
          position: 'absolute',
          left: 0,
          right: 0,
          top: Math.floor(height / 2) - 3,
          height: 6,
          boxSizing: 'border-box',
          border: '1px solid #606060',
          background: '#d8d8d8',
        }}
      >
        <div style={{ width: left + SLIDER_THUMB / 2, height: '100%', background: s.color }} />
      </div>
      <div
        data-testid="channel-thumb"
        style={{
          position: 'absolute',
          left,
          top: 1,
          width: SLIDER_THUMB,
          height: height - 2,
          boxSizing: 'border-box',
          border: '1px solid #404040',
          background: s.dragging ? '#ffd9a0' : '#f0f0f0',
        }}
      />
    </div>
  );
}
