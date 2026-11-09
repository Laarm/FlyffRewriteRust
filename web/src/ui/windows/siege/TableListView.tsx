// The pixels of a `TableList`: the two rules (black, alpha 128), the header, the visible rows (the selected one behind a black bar of alpha 64) and the extra own row.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { FONT, fontStyle } from '../../text/fonts';
import { cssColor } from '../../text/markup';
import { ControlHost } from '../../wnd/FrameWindowView';
import type { TableList, TableRow } from './TableList';

/** The rule rects of `OnDraw`: `(sx, 5, sx + w, 7)` and the same 20 px lower. */
export const RULE_Y = [5, 25] as const;
export const RULE_HEIGHT = 2;
/** `sy = 35` and `sy += 18`. */
export const TABLE_TOP = 35;
export const TABLE_PITCH = 18;
/** `GetSelectIndex`: `(point.y - 32) / 18`. */
export const TABLE_HIT_TOP = 32;

function Row({ row, y }: { row: TableRow; y: number }) {
  return (
    <>
      {row.cells.map((c, i) => (
        <div
          key={i}
          data-testid="table-cell"
          style={{
            position: 'absolute',
            left: c.x,
            top: y,
            whiteSpace: 'pre',
            color: cssColor(row.color),
            pointerEvents: 'none',
            ...fontStyle(FONT.text),
          }}
        >
          {c.text}
        </div>
      ))}
    </>
  );
}

export function TableListView({ control }: { control: TableList }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const spec = control.spec;
  const bar = control.scrollBar;
  const last = Math.min(s.rows.length, s.scroll + s.page);
  const rows = [];
  for (let i = Math.max(0, s.scroll); i < last; i += 1) {
    const row = s.rows[i];
    if (!row) continue;
    const y = TABLE_TOP + (i - s.scroll) * TABLE_PITCH;
    rows.push(
      <div key={i} data-testid="table-row" data-row={i}>
        {i === s.selected && (
          <div
            data-testid="table-select"
            style={{
              position: 'absolute',
              left: spec.left,
              top: y - 4,
              width: spec.selectWidth,
              height: 20,
              background: 'rgba(0, 0, 0, 0.251)',
              pointerEvents: 'none',
            }}
          />
        )}
        <Row row={row} y={y} />
      </div>,
    );
  }
  const rule = (y: number) => (
    <div
      key={y}
      data-testid="table-rule"
      style={{
        position: 'absolute',
        left: spec.left,
        top: y,
        width: spec.ruleWidth,
        height: RULE_HEIGHT,
        background: 'rgba(0, 0, 0, 0.502)',
        pointerEvents: 'none',
      }}
    />
  );
  return (
    <div
      className="ig"
      data-testid="table-list"
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
        {RULE_Y.map(rule)}
        {s.header.map((h, i) => (
          <div
            key={i}
            data-testid="table-header"
            style={{
              position: 'absolute',
              left: h.x,
              top: h.y,
              whiteSpace: 'pre',
              color: cssColor(h.color),
              pointerEvents: 'none',
              ...fontStyle(FONT.text),
            }}
          >
            {h.text}
          </div>
        ))}
        {rows}
        {s.own && spec.ownY !== undefined && (
          <div data-testid="table-own">
            <Row row={s.own} y={spec.ownY} />
          </div>
        )}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
