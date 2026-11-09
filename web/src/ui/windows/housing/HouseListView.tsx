// The pixels of a `HouseList`: the visible rows (texts at their offsets, an optional item icon, the selection fill behind the selected row) and the docked scroll bar.

import { useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { FONT, fontStyle } from '../../text/fonts';
import { cssColor } from '../../text/markup';
import type { WndEnv } from '../../wnd/env';
import { ControlHost } from '../../wnd/FrameWindowView';
import type { HouseList, HouseRow } from './HouseList';

function Icon({ env, path, x, y }: { env: WndEnv; path: string | null; x: number; y: number }) {
  const sprite = useSprite(env, path);
  if (typeof sprite !== 'object') return null;
  return (
    <div
      data-testid="house-icon"
      data-path={path ?? ''}
      style={{
        position: 'absolute',
        left: x,
        top: y,
        width: sprite.width,
        height: sprite.height,
        backgroundImage: `url("${sprite.url}")`,
        backgroundRepeat: 'no-repeat',
        pointerEvents: 'none',
      }}
    />
  );
}

function Row({
  env,
  row,
  y,
  selected,
}: {
  env: WndEnv;
  row: HouseRow;
  y: number;
  selected: boolean;
}) {
  const color = selected && row.selectedColor !== undefined ? row.selectedColor : row.color;
  return (
    <div
      data-testid="house-row"
      data-key={String(row.key)}
      data-selected={selected}
      style={{ position: 'absolute', left: 0, top: y, width: 0, height: 0 }}
    >
      {row.icon && <Icon env={env} path={row.icon.path} x={row.icon.x} y={row.icon.y} />}
      {row.cells.map((c, i) => (
        <div
          key={i}
          data-testid="house-cell"
          style={{
            position: 'absolute',
            left: c.x,
            top: c.y,
            whiteSpace: 'pre',
            color: cssColor(color),
            pointerEvents: 'none',
            ...fontStyle(FONT.text),
          }}
        >
          {c.text}
        </div>
      ))}
    </div>
  );
}

export function HouseListView({ control }: { control: HouseList }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const spec = s.spec;
  const bar = control.scrollBar;
  const last = Math.min(s.rows.length, s.scroll + s.page);
  const rows = [];
  let fill = null;
  for (let i = Math.max(0, s.scroll); i < last; i += 1) {
    const row = s.rows[i];
    if (!row) continue;
    const y = spec.top + (i - s.scroll) * spec.pitch;
    const selected = i === s.selected;
    if (selected && spec.selectFill) {
      const f = spec.selectFill;
      fill = (
        <div
          key="fill"
          data-testid="house-select"
          style={{
            position: 'absolute',
            left: f.left,
            top: y + f.yOffset,
            width: Math.max(0, rectWidth(w) - f.left - f.right),
            height: f.height,
            background: f.color,
            pointerEvents: 'none',
          }}
        />
      );
    }
    rows.push(<Row key={row.key} env={control.env} row={row} y={y} selected={selected} />);
  }
  return (
    <div
      className="ig"
      data-testid="house-list"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      <div
        className="ig-client"
        style={{
          left: c.l - w.l,
          top: c.t - w.t,
          width: Math.max(0, rectWidth(c)),
          height: Math.max(0, rectHeight(c)),
          overflow: 'visible',
        }}
      >
        {fill}
        {rows}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
