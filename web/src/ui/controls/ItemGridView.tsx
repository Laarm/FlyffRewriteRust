// The cells of an item grid (`CWndItemCtrl::OnDraw`): the visible rows from the scroll position, one `IconCell` per
// occupied slot at (col * 32, row * 32) of the client rect, and the docked scroll bar.

import { useSyncExternalStore } from 'react';
import { IconCell } from '../icons/IconCell';
import { rectHeight, rectWidth } from '../layout/geometry';
import { ControlHost } from '../wnd/FrameWindowView';
import type { ItemGrid } from './ItemGrid';
import { CELL } from './ItemGrid';

export function ItemGridView({ control }: { control: ItemGrid }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const first = s.scroll * s.columns;
  // one extra (partial) row below the page is clipped by the control
  const last = Math.min(s.size, first + s.columns * (s.rows + 1));
  const cells = [];
  for (let slot = first; slot < last; slot += 1) {
    const cell = s.cells[slot];
    if (!cell) continue;
    const i = slot - first;
    cells.push(
      <IconCell
        key={slot}
        env={control.env}
        cell={cell}
        slot={slot}
        x={(i % s.columns) * CELL}
        y={Math.floor(i / s.columns) * CELL}
        newFontColor={s.newFontColor}
        hover={s.hover === slot}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="item-grid"
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
        {cells}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
