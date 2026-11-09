// `FinderList`: the rows of the party finder (`CWndListBox::OnDraw` special case of `APP_PARTY_FIND`, `src:_Interface/WndControl.cpp:1884-2040`).
// 20 px rows: the index, the party name, the leader, the member count `n/8`, the level, the points, and six 16 x 16 icons for the modes that
// run (the party-skill-full item, the EXP-up item, the skill-up item, Link Attack, Fortune Circle, Gift Box). A press selects a row (blue
// text); the wheel and the docked bar scroll. The control is a `RowList`: the shell pushes the rows and reads the selection.

import { memo, useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { iconImageStyle } from '../../icons/IconCell';
import type { Point } from '../../layout/geometry';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { visibleRows } from '../../lists/geometry';
import { RowList, type RowListState } from '../../lists/RowList';
import { viewOf } from '../../controls/bind';
import { FONT, fontStyle } from '../../text/fonts';
import type { WndEnv } from '../../wnd/env';
import { ControlHost } from '../../wnd/FrameWindowView';
import { Wnm } from '../../layout/styles';
import type { FinderRow } from './ports';

/** `m_nFontHeight = 20`. */
export const FINDER_ROW_PITCH = 20;
/** The x of the columns (`pt.x += 32 / 109 / 109 / 66 / 66 / 66`) and of the icons (18, 18, 18, 15, 15). */
export const FINDER_COL = {
  index: 2,
  name: 34,
  leader: 143,
  size: 252,
  level: 318,
  points: 384,
} as const;
export const FINDER_ICON_X = [450, 468, 486, 504, 519, 534] as const;
/** The icons are drawn at half scale. */
export const FINDER_ICON_SIZE = 16;
const TEXT = '#000';
/** `D3DCOLOR_XRGB( 97, 97, 255 )`: the selected row. */
const SELECTED = 'rgb(97,97,255)';

export interface FinderListDelegate {
  /** The selection changed (a press, `-1` when it is cleared). */
  select?(row: FinderRow | null, index: number): void;
  /** A double click on a row. */
  activate?(row: FinderRow, index: number): void;
}

export interface FinderListState extends RowListState<FinderRow> {
  /** The icon files of the six flags. */
  icons: readonly (string | null)[];
}

export class FinderList extends RowList<FinderRow, FinderListState> {
  #delegate: FinderListDelegate;

  constructor(delegate: FinderListDelegate = {}) {
    super(
      { pitch: FINDER_ROW_PITCH, top: 2, inset: 0, bar: true, frame: false },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: 1,
        clientOffset: { x: 0, y: 0 },
        icons: [],
      },
    );
    this.#delegate = delegate;
    this.view = viewOf<FinderList>(FinderListView);
  }

  setDelegate(delegate: FinderListDelegate): void {
    this.#delegate = delegate;
  }

  /** The row list of a refresh: the selection is cleared (`RefreshItemList` sets `m_nSelected = 0`). */
  setRows(rows: readonly FinderRow[], icons: readonly (string | null)[]): void {
    this.state.patch({ icons, selected: -1 });
    this.setRowsState(rows, { selected: -1 });
  }

  clear(): void {
    this.setRowsState([], { selected: -1 });
  }

  get selectedFinderRow(): FinderRow | null {
    const i = this.selectedRow;
    return i >= 0 ? (this.rows[i] ?? null) : null;
  }

  override onLButtonDown(pt: Point): void {
    const i = this.rowAt(pt);
    if (i < 0) return;
    this.select(i);
    this.#delegate.select?.(this.rows[i] ?? null, i);
    this.parent?.onChildNotify(Wnm.SELCHANGE, this.id, this);
  }

  override onLButtonDblClk(pt: Point): void {
    const i = this.rowAt(pt);
    const row = i >= 0 ? this.rows[i] : undefined;
    if (row) this.#delegate.activate?.(row, i);
  }
}

const Icon = memo(function Icon({ env, path, x }: { env: WndEnv; path: string | null; x: number }) {
  const sprite = useSprite(env, path);
  return (
    <div
      data-testid="finder-icon"
      style={iconImageStyle({
        url: typeof sprite === 'object' ? sprite.url : null,
        size: FINDER_ICON_SIZE,
        left: x,
        top: 1,
        alpha: 1,
        tint: false,
      })}
    />
  );
});

const Row = memo(function Row({
  env,
  row,
  index,
  icons,
  y,
  selected,
}: {
  env: WndEnv;
  row: FinderRow;
  index: number;
  icons: readonly (string | null)[];
  y: number;
  selected: boolean;
}) {
  const color = selected ? SELECTED : TEXT;
  const cell = (id: string, x: number, text: string) => (
    <div
      key={id}
      data-testid={`finder-${id}`}
      style={{
        position: 'absolute',
        left: x,
        top: 0,
        color,
        whiteSpace: 'pre',
        ...fontStyle(FONT.text),
      }}
    >
      {text}
    </div>
  );
  return (
    <div
      data-testid="finder-row"
      data-key={String(row.partyId)}
      data-selected={selected}
      style={{ position: 'absolute', left: 0, top: y, width: '100%', height: FINDER_ROW_PITCH }}
    >
      {cell('index', FINDER_COL.index, String(index))}
      {cell('name', FINDER_COL.name, row.name)}
      {cell('leader', FINDER_COL.leader, row.leaderName)}
      {cell('size', FINDER_COL.size, row.sizeText)}
      {cell('level', FINDER_COL.level, String(row.level))}
      {cell('points', FINDER_COL.points, String(row.point))}
      {row.flags.map((on, k) =>
        on ? <Icon key={k} env={env} path={icons[k] ?? null} x={FINDER_ICON_X[k] ?? 0} /> : null,
      )}
    </div>
  );
});

export function FinderListView({ control }: { control: FinderList }) {
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
        key={row.partyId}
        env={control.env}
        row={row}
        index={i}
        icons={s.icons}
        y={control.geometry.top + (i - s.scroll) * FINDER_ROW_PITCH}
        selected={s.selected === i}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="finder-list"
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
