// The rows of the inbox (`CWndPostReceive::OnDraw`): the table picture, then per row the selection fill, the icon with its count, the "read" mark, the
// sender, the time that is left and the title. Positions are those of the original (`sx = 9`, `sy = 30` plus 40 per row).

import { memo, useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { iconImageStyle } from '../../icons/IconCell';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { visibleRows } from '../../lists/geometry';
import { FONT, fontStyle } from '../../text/fonts';
import type { WndEnv } from '../../wnd/env';
import { ControlHost } from '../../wnd/FrameWindowView';
import {
  MAIL_GOLD_ICON,
  MAIL_READ_ALPHA,
  MAIL_READ_TEXTURE,
  MAIL_ROW_LEFT,
  MAIL_ROW_PITCH,
  MAIL_ROW_RIGHT,
  MAIL_ROW_TOP,
  MAIL_TABLE_ALPHA,
  MAIL_TABLE_TEXTURE,
  type MailList,
} from './MailList';
import type { MailRow } from './ports';

const css = (argb: number): string => {
  const r = (argb >>> 16) & 255;
  const g = (argb >>> 8) & 255;
  const b = argb & 255;
  return `rgb(${String(r)}, ${String(g)}, ${String(b)})`;
};

const text = (x: number, y: number, color: string): React.CSSProperties => ({
  position: 'absolute',
  left: x,
  top: y,
  color,
  whiteSpace: 'pre',
  ...fontStyle(FONT.text),
});

const Row = memo(function Row({
  env,
  row,
  y,
  selected,
  hover,
  newFontColor,
}: {
  env: WndEnv;
  row: MailRow;
  y: number;
  selected: boolean;
  hover: boolean;
  newFontColor: boolean;
}) {
  const iconPath = row.item ? row.item.icon : row.gold > 0 ? MAIL_GOLD_ICON : null;
  const icon = useSprite(env, iconPath);
  const mark = useSprite(env, row.read ? MAIL_READ_TEXTURE : null);
  const count = row.item?.stackable === true ? row.item.count : null;
  return (
    <div
      data-testid="mail-row"
      data-mail={row.id}
      data-read={row.read}
      data-hover={hover}
      style={{
        position: 'absolute',
        left: 0,
        top: y,
        width: MAIL_ROW_RIGHT,
        height: MAIL_ROW_PITCH,
      }}
    >
      {selected && (
        <div
          data-testid="mail-selection"
          style={{
            position: 'absolute',
            left: MAIL_ROW_LEFT,
            top: 0,
            width: MAIL_ROW_RIGHT - MAIL_ROW_LEFT,
            height: MAIL_ROW_PITCH,
            background: 'rgba(190, 0, 0, 0.125)',
          }}
        />
      )}
      <div
        data-testid="mail-icon"
        style={iconImageStyle({
          url: typeof icon === 'object' ? icon.url : null,
          size: 32,
          left: 19,
          top: 6,
          alpha: 1,
          tint: false,
        })}
      />
      {count !== null && (
        <div
          data-testid="mail-count"
          style={{
            ...text(36, 24, newFontColor ? '#eaebdd' : '#b0b0f0'),
            ...fontStyle(FONT.title),
          }}
        >
          {String(count)}
        </div>
      )}
      {row.read && (
        <div
          data-testid="mail-read"
          style={iconImageStyle({
            url: typeof mark === 'object' ? mark.url : null,
            size: 32,
            left: 19,
            top: 5,
            alpha: MAIL_READ_ALPHA,
            tint: false,
          })}
        />
      )}
      <div data-testid="mail-sender" style={text(69, 7, '#000')}>
        {row.sender}
      </div>
      <div data-testid="mail-keep" style={text(row.keepOver ? 199 : 202, 7, css(row.keepColor))}>
        {row.keepText}
      </div>
      <div data-testid="mail-title" style={text(69, 22, 'rgb(0, 0, 190)')}>
        {row.title}
      </div>
    </div>
  );
});

export function MailListView({ control }: { control: MailList }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const table = useSprite(control.env, MAIL_TABLE_TEXTURE);
  const w = control.rectWindow;
  const c = control.rectClient;
  const [first, last] = visibleRows(s.scroll, s.page, s.rows.length);
  const rows = [];
  for (let i = first; i < last; i += 1) {
    const row = s.rows[i];
    if (!row) continue;
    rows.push(
      <Row
        key={row.id}
        env={control.env}
        row={row}
        y={MAIL_ROW_TOP + (i - s.scroll) * MAIL_ROW_PITCH}
        selected={s.selected === i}
        hover={s.hover === i}
        newFontColor={s.newFontColor}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="mail-list"
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
        {typeof table === 'object' && (
          <div
            data-testid="mail-table"
            style={{
              position: 'absolute',
              left: MAIL_ROW_LEFT,
              top: MAIL_ROW_TOP,
              width: table.width,
              height: table.height,
              backgroundImage: `url("${table.url}")`,
              backgroundRepeat: 'no-repeat',
              opacity: MAIL_TABLE_ALPHA,
              pointerEvents: 'none',
            }}
          />
        )}
        {rows}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
