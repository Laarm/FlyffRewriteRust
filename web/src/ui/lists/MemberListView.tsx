// The pixels of a member list: the header strip with the sort buttons, then only the visible rows. A row is a memoised
// component keyed by the member and compared by the identity of its (merged) row object plus its selected / hover flags, so a
// 10 Hz update of one value re-renders one row, not 200.

import { memo, useSyncExternalStore } from 'react';
import { useSprite } from '../controls/useSprite';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, fontStyle, lineHeight } from '../text/fonts';
import { cssColor } from '../text/markup';
import type { WndEnv } from '../wnd/env';
import { ControlHost } from '../wnd/FrameWindowView';
import { visibleRows } from './geometry';
import type { MemberColumn, MemberRow, MemberSort } from './memberModel';
import type { MemberList } from './MemberList';
import { PaintedFrame } from './SlotGridView';

const TEXT = 0xff000000;
const SELECT_BG = 'rgba(96, 96, 255, 0.35)';
const HOVER_BG = 'rgba(255, 128, 0, 0.18)';

function IconCellView({
  env,
  icon,
  x,
  y,
}: {
  env: WndEnv;
  icon: string | null | undefined;
  x: number;
  y: number;
}) {
  const sprite = useSprite(env, icon ?? null);
  if (typeof sprite !== 'object') return null;
  return (
    <div
      data-testid="member-icon"
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

const Row = memo(function Row({
  env,
  row,
  columns,
  y,
  width,
  height,
  selected,
  hover,
}: {
  env: WndEnv;
  row: MemberRow;
  columns: readonly MemberColumn[];
  y: number;
  width: number;
  height: number;
  selected: boolean;
  hover: boolean;
}) {
  const cell = lineHeight(FONT.text);
  const cells = [];
  let left = 0;
  for (const col of columns) {
    const c = row.cells[col.id];
    const x = left;
    left += col.width;
    if (!c) continue;
    if (col.kind === 'icon') {
      cells.push(
        <IconCellView
          key={col.id}
          env={env}
          icon={c.icon}
          x={x + 2}
          y={Math.max(0, (height - 16) / 2)}
        />,
      );
      continue;
    }
    const align = col.align ?? 'left';
    cells.push(
      <div
        key={col.id}
        data-testid="member-cell"
        data-col={col.id}
        style={{
          position: 'absolute',
          left: x,
          width: col.width,
          top: Math.max(0, Math.floor((height - cell) / 2)),
          textAlign: align,
          boxSizing: 'border-box',
          padding: '0 4px',
          color: cssColor(c.color ?? row.color ?? TEXT),
          whiteSpace: 'pre',
          overflow: 'hidden',
          ...fontStyle(FONT.text),
        }}
      >
        {c.text}
      </div>,
    );
  }
  return (
    <div
      data-testid="member-row"
      data-key={String(row.key)}
      data-selected={selected}
      data-hover={hover}
      style={{
        position: 'absolute',
        left: 0,
        top: y,
        width,
        height,
        opacity: row.dim === true ? 0.55 : 1,
        background: selected ? SELECT_BG : hover ? HOVER_BG : 'transparent',
      }}
    >
      {cells}
      {row.gauge !== undefined && (
        <div
          data-testid="member-gauge"
          style={{
            position: 'absolute',
            left: 20,
            top: height - 8,
            width: Math.max(0, width - 40),
            height: 6,
            boxSizing: 'border-box',
            border: '1px solid #606060',
            background: '#d8d8d8',
          }}
        >
          <div
            style={{
              width: `${String(Math.round(Math.max(0, Math.min(1, row.gauge)) * 100))}%`,
              height: '100%',
              background: cssColor(row.gaugeColor ?? 0x64ff0000),
            }}
          />
        </div>
      )}
    </div>
  );
});

function Header({
  columns,
  sort,
  height,
}: {
  columns: readonly MemberColumn[];
  sort: MemberSort | null;
  height: number;
}) {
  const cells = [];
  let left = 0;
  for (const col of columns) {
    const sorted = sort?.column === col.id;
    cells.push(
      <div
        key={col.id}
        data-testid="member-header"
        data-col={col.id}
        data-sortable={col.sortable === true}
        data-sorted={sorted ? (sort.dir === 1 ? 'asc' : 'desc') : undefined}
        style={{
          position: 'absolute',
          left,
          top: 0,
          width: col.width,
          height,
          boxSizing: 'border-box',
          border: '1px solid #8a8a8a',
          background: col.sortable === true ? '#e4e4e4' : '#d0d0d0',
          textAlign: col.align ?? 'left',
          padding: '0 4px',
          color: '#000',
          whiteSpace: 'pre',
          overflow: 'hidden',
          ...fontStyle(FONT.text),
        }}
      >
        {col.title}
        {sorted ? (sort.dir === 1 ? ' ▲' : ' ▼') : ''}
      </div>,
    );
    left += col.width;
  }
  return <>{cells}</>;
}

export function MemberListView({ control }: { control: MemberList }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const pitch = control.geometry.pitch;
  const header = control.geometry.header ?? 0;
  const [first, last] = visibleRows(s.scroll, s.page, s.rows.length);
  const selected = new Set(s.selection);
  const rows = [];
  for (let i = first; i < last; i += 1) {
    const row = s.rows[i];
    if (!row) continue;
    rows.push(
      <Row
        key={row.key}
        env={control.env}
        row={row}
        columns={s.columns}
        y={control.rowsTop + (i - s.scroll) * pitch}
        width={rectWidth(c)}
        height={pitch}
        selected={selected.has(row.key)}
        hover={s.hover === i}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="member-list"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      {control.geometry.frame && <PaintedFrame width={rectWidth(w)} height={rectHeight(w)} />}
      <div
        className="ig-client"
        style={{
          left: c.l - w.l,
          top: c.t - w.t,
          width: Math.max(0, rectWidth(c)),
          height: Math.max(0, rectHeight(c)),
        }}
      >
        {header > 0 && <Header columns={s.columns} sort={s.sort} height={header} />}
        {rows}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
