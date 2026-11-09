// The pixels of a bank log page: only the visible lines, 20 px apart, black text at x 4.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { visibleRows } from '../../lists/geometry';
import { FONT, fontStyle } from '../../text/fonts';
import { cssColor } from '../../text/markup';
import { ControlHost } from '../../wnd/FrameWindowView';
import type { BankLogList } from './BankLogList';

/** `m_nFontHeight`. */
export const LOG_PITCH = 20;
/** `TextOut( 4, y, ...)` with `pt( 3, 3 )`. */
export const LOG_X = 4;
export const LOG_TOP = 3;

const TEXT = 0xff000000;

export function BankLogListView({ control }: { control: BankLogList }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const [first, last] = visibleRows(s.scroll, s.page, s.rows.length);
  const lines = [];
  for (let i = first; i < last; i += 1) {
    const text = s.rows[i];
    if (text === undefined) continue;
    lines.push(
      <div
        key={i}
        data-testid="bank-log-line"
        style={{
          position: 'absolute',
          left: LOG_X,
          top: LOG_TOP + (i - s.scroll) * LOG_PITCH,
          whiteSpace: 'pre',
          color: cssColor(TEXT),
          pointerEvents: 'none',
          ...fontStyle(FONT.text),
        }}
      >
        {text}
      </div>,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="bank-log-list"
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
        {lines}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
