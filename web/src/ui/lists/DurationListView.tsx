// The rows of the duration list (`CWndCommItemCtrl::DrawSM`): icon at alpha 192, the blue name, the remaining time.

import { memo, useSyncExternalStore } from 'react';
import { useSprite } from '../controls/useSprite';
import { iconImageStyle } from '../icons/IconCell';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, fontStyle } from '../text/fonts';
import type { WndEnv } from '../wnd/env';
import { ControlHost } from '../wnd/FrameWindowView';
import { visibleRows } from './geometry';
import { type DurationList, type DurationRow, DURATION_ROW_PITCH } from './DurationList';

const Row = memo(function Row({
  env,
  row,
  y,
  width,
  hover,
}: {
  env: WndEnv;
  row: DurationRow;
  y: number;
  width: number;
  hover: boolean;
}) {
  const sprite = useSprite(env, row.icon);
  return (
    <div
      data-testid="duration-row"
      data-hover={hover}
      style={{ position: 'absolute', left: 0, top: y, width, height: DURATION_ROW_PITCH }}
    >
      <div
        data-testid="duration-icon"
        style={iconImageStyle({
          url: typeof sprite === 'object' ? sprite.url : null,
          size: 32,
          left: 2,
          top: 0,
          alpha: 192 / 255,
          tint: false,
        })}
      />
      <div
        data-testid="duration-name"
        style={{
          position: 'absolute',
          left: 40,
          top: 3,
          color: '#0000ff',
          whiteSpace: 'pre',
          ...fontStyle(FONT.text),
        }}
      >
        {row.name}
      </div>
      <div
        data-testid="duration-text"
        style={{
          position: 'absolute',
          left: 40,
          top: 18,
          color: '#000',
          whiteSpace: 'pre',
          ...fontStyle(FONT.text),
        }}
      >
        {row.text}
      </div>
    </div>
  );
});

export function DurationListView({ control }: { control: DurationList }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const [first, last] = visibleRows(s.scroll, s.page, s.rows.length);
  const rows = [];
  for (let i = first; i < last; i += 1) {
    const row = s.rows[i];
    if (!row) continue;
    rows.push(
      <Row
        key={row.key}
        env={control.env}
        row={row}
        y={control.geometry.top + (i - s.scroll) * DURATION_ROW_PITCH}
        width={rectWidth(c)}
        hover={s.hover === i}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="duration-list"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      <div
        className="ig-client"
        style={{
          left: c.l - w.l,
          top: c.t - w.t,
          width: Math.max(0, rectWidth(c)),
          height: Math.max(0, rectHeight(c)),
        }}
      >
        {rows}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
