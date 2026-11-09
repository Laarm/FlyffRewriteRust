// The rows of a list box (`CWndListBox::OnDraw/PaintListBox`): the visible items from the scroll position, each one
// line at (3 + margin, 3 + row * pitch) of the client rect, recoloured by state (selected / hover / invalid / normal).

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, lineHeight } from '../text/fonts';
import { RichText } from '../text/RichText';
import type { RichLine } from '../text/layout';
import { ControlHost } from '../wnd/FrameWindowView';
import type { ListBox } from './ListBox';

export function ListBoxView({ control }: { control: ListBox }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const bar = control.scrollBar;
  const pitch = control.pitch();
  const rows: { key: number; top: number; line: RichLine }[] = [];
  let row = 0;
  for (let i = 0; i < s.items.length; i += 1) {
    const item = s.items[i];
    if (!item?.visible) continue;
    const y = row - s.scroll;
    row += 1;
    if (y < 0 || (y + 1) * pitch > rectHeight(c) + pitch) continue;
    const color = !item.valid
      ? s.invalidColor
      : i === s.cur
        ? s.selectColor
        : i === s.hover
          ? s.hoverColor
          : s.fontColor;
    rows.push({
      key: i,
      top: 3 + y * pitch,
      line: { runs: item.runs.map((r) => ({ ...r, color })), width: 0, offset: 0, length: 0 },
    });
  }
  return (
    <div
      className="lb"
      data-testid="listbox"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      <div
        className="lb-client"
        style={{
          left: c.l - w.l,
          top: c.t - w.t,
          width: Math.max(0, rectWidth(c)),
          height: Math.max(0, rectHeight(c)),
        }}
      >
        {rows.map((r) => (
          <RichText
            key={r.key}
            lines={[r.line]}
            font={FONT.text}
            className="lb-row"
            style={{
              left: 3 + s.leftMargin,
              top: r.top,
              height: lineHeight(FONT.text),
            }}
          />
        ))}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
