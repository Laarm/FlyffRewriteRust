// The pixels of a messenger list: the translucent frame, then only the visible rows, each with its pictures and texts at the positions of the original's
// `OnDraw` (`icons.ts` `COL`). A row is a memoised component compared by the identity of its merged row object plus its selected flag, so a state change of
// one friend re-renders one row.

import { memo, useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { visibleRows } from '../../lists/geometry';
import type { MemberRow } from '../../lists/memberModel';
import { PaintedFrame } from '../../lists/SlotGridView';
import { spriteRect } from '../../skin/atlas';
import { FONT, fontStyle } from '../../text/fonts';
import { cssColor } from '../../text/markup';
import type { WndEnv } from '../../wnd/env';
import { ControlHost } from '../../wnd/FrameWindowView';
import { COL, NAME_COLOR, parseSpriteRef, PLAYER_DATA_ATLAS, ROW_PITCH, TEXT_DY } from './icons';
import type { MessengerList } from './MessengerList';

/** One sprite of `icon_PlayerData.inc`, cut out of its texture at natural size. */
function AtlasIcon({ env, icon, x }: { env: WndEnv; icon: string | null | undefined; x: number }) {
  const index = parseSpriteRef(icon);
  const dto = index === null ? undefined : env.host.bundles.atlases?.atlases[PLAYER_DATA_ATLAS];
  const sprite = useSprite(env, dto?.texture ?? null);
  const s = index === null ? undefined : dto?.sprites[index];
  if (!s || typeof sprite !== 'object') return null;
  const [sx, sy, w, h] = spriteRect(s, sprite.width, sprite.height);
  return (
    <div
      data-testid="messenger-icon"
      data-sprite={index}
      style={{
        position: 'absolute',
        left: x,
        top: 0,
        width: w,
        height: h,
        backgroundImage: `url("${sprite.url}")`,
        backgroundSize: `${String(sprite.width)}px ${String(sprite.height)}px`,
        backgroundPosition: `${String(-sx)}px ${String(-sy)}px`,
        backgroundRepeat: 'no-repeat',
        pointerEvents: 'none',
      }}
    />
  );
}

function Text({ x, text, color }: { x: number; text: string; color: number }) {
  return (
    <div
      data-testid="messenger-text"
      style={{
        position: 'absolute',
        left: x,
        top: TEXT_DY,
        whiteSpace: 'pre',
        color: cssColor(color),
        pointerEvents: 'none',
        ...fontStyle(FONT.text),
      }}
    >
      {text}
    </div>
  );
}

const Row = memo(function Row({
  env,
  row,
  y,
  width,
  selected,
}: {
  env: WndEnv;
  row: MemberRow;
  y: number;
  width: number;
  selected: boolean;
}) {
  const name = row.cells['name'];
  const level = row.cells['level'];
  const nameColor = name?.color ?? (selected ? NAME_COLOR.selected : NAME_COLOR.normal);
  // a blocked name stays red under the selection, a visitor stays green (`dwColor` of `OnDraw`)
  return (
    <div
      data-testid="messenger-row"
      data-key={String(row.key)}
      data-selected={selected}
      style={{ position: 'absolute', left: 0, top: y, width, height: ROW_PITCH }}
    >
      <AtlasIcon env={env} icon={row.cells['channel']?.icon} x={COL.channel} />
      <AtlasIcon env={env} icon={row.cells['state']?.icon} x={COL.state} />
      <Text x={COL.level} text={level?.text ?? ''} color={level?.color ?? NAME_COLOR.normal} />
      <AtlasIcon env={env} icon={row.cells['jobType']?.icon} x={COL.jobType} />
      <AtlasIcon env={env} icon={row.cells['job']?.icon} x={COL.job} />
      <Text x={COL.name} text={name?.text ?? ''} color={nameColor} />
    </div>
  );
});

export function MessengerListView({ control }: { control: MessengerList }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
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
        y={control.rowsTop + (i - s.scroll) * ROW_PITCH}
        width={rectWidth(c)}
        selected={selected.has(row.key)}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="messenger-list"
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
        {rows}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
