// The pixels of a slot grid: the optional translucent frame (`PaintFrame`), one `IconCell` per occupied visible slot at the
// origin the layout gives it, the selection rectangle and the docked scroll bar. Subclasses add what stands beside an icon
// (the vendor's name and price) through `extras`.

import { type ReactNode, useSyncExternalStore } from 'react';
import { IconCell, type IconCellData } from '../icons/IconCell';
import { rectHeight, rectWidth } from '../layout/geometry';
import { ControlHost } from '../wnd/FrameWindowView';
import type { SlotGrid } from './SlotGrid';

/** `PaintFrame` of the three controls: fill `ARGB(100, 0, 0, 0)`, outlines `0xFFF0F0F0` twice and `ARGB(100, 200, 200, 200)`. */
export function PaintedFrame({ width, height }: { width: number; height: number }) {
  return (
    <div
      data-testid="slot-frame"
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        width,
        height,
        boxSizing: 'border-box',
        background: 'rgba(0, 0, 0, 0.39)',
        border: '1px solid #f0f0f0',
        borderRadius: 2,
        pointerEvents: 'none',
        boxShadow: 'inset 0 0 0 1px #f0f0f0, inset 0 0 0 2px rgba(200, 200, 200, 0.39)',
      }}
    />
  );
}

export type SlotExtras = (
  slot: number,
  cell: IconCellData,
  origin: { x: number; y: number },
) => ReactNode;

export function SlotGridBody({
  control,
  extras,
}: {
  control: SlotGrid;
  extras?: SlotExtras | undefined;
}) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const { layout } = control.options;
  const w = control.rectWindow;
  const c = control.rectClient;
  const first = s.scroll * layout.columns;
  const last = Math.min(layout.count, first + layout.columns * (s.page + 1));
  const cells: ReactNode[] = [];
  const [sx, sy, sw, sh] = control.options.selectRect;
  for (let slot = first; slot < last; slot += 1) {
    const cell = s.cells[slot];
    if (!cell) continue;
    const o = control.slotOrigin(slot);
    cells.push(
      <div key={slot} data-slot-group={slot}>
        <IconCell
          env={control.env}
          cell={cell}
          slot={slot}
          x={o.x}
          y={o.y}
          newFontColor={control.options.newFontColor}
          hover={s.hover === slot}
        />
        {extras?.(slot, cell, o)}
        {s.selected === slot && (
          <div
            data-testid="slot-select"
            style={{
              position: 'absolute',
              left: o.x + sx,
              top: o.y + sy,
              width: sw,
              height: sh,
              boxSizing: 'border-box',
              border: `1px solid ${control.options.selectColor}`,
              pointerEvents: 'none',
            }}
          />
        )}
      </div>,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="slot-grid"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      {control.options.frame && <PaintedFrame width={rectWidth(w)} height={rectHeight(w)} />}
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

export function SlotGridView({ control }: { control: SlotGrid }) {
  return <SlotGridBody control={control} />;
}
