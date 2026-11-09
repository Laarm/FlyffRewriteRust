// The pixels of the guild member list: only the visible rows, each with its rank marks, job marks and texts at the positions of the original's `OnDraw`
// (`memberIcons.ts` `COL`). A row is a memoised component compared by the identity of its row object plus its selected flag, so a change of one member re-renders
// one row. The pictures of an offline member are tinted (255, 100, 100) by an SVG colour matrix (the original multiplied the vertex colour `0xffff6464`).

import { memo, useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { visibleRows } from '../../lists/geometry';
import { spriteRect } from '../../skin/atlas';
import { FONT, fontStyle } from '../../text/fonts';
import { cssColor } from '../../text/markup';
import type { WndEnv } from '../../wnd/env';
import { ControlHost } from '../../wnd/FrameWindowView';
import type { GuildMemberList } from './GuildMemberList';
import {
  COL,
  cutMemberName,
  JOB_DY,
  memberJobMarks,
  PLAYER_DATA_ATLAS,
  RANK_DY,
  ROW_PITCH,
  ROWS_TOP,
  rankMarks,
  TEXT_COLOR,
} from './memberIcons';
import type { GuildMemberRow } from './ports';

/** The id of the SVG filter that multiplies the colour by (255, 100, 100). */
export const OFFLINE_FILTER_ID = 'flyff-guild-offline-tint';

/** One sprite of `icon_PlayerData.inc`, cut out of its texture at natural size. */
function AtlasIcon({
  env,
  sprite,
  x,
  y,
  tint,
}: {
  env: WndEnv;
  sprite: number | null;
  x: number;
  y: number;
  tint: boolean;
}) {
  const dto = sprite === null ? undefined : env.host.bundles.atlases?.atlases[PLAYER_DATA_ATLAS];
  const tex = useSprite(env, dto?.texture ?? null);
  const s = sprite === null ? undefined : dto?.sprites[sprite];
  if (!s || typeof tex !== 'object') return null;
  const [sx, sy, w, h] = spriteRect(s, tex.width, tex.height);
  return (
    <div
      data-testid="guild-icon"
      data-sprite={sprite}
      data-tint={tint}
      style={{
        position: 'absolute',
        left: x,
        top: y,
        width: w,
        height: h,
        backgroundImage: `url("${tex.url}")`,
        backgroundSize: `${String(tex.width)}px ${String(tex.height)}px`,
        backgroundPosition: `${String(-sx)}px ${String(-sy)}px`,
        backgroundRepeat: 'no-repeat',
        pointerEvents: 'none',
        ...(tint ? { filter: `url(#${OFFLINE_FILTER_ID})` } : {}),
      }}
    />
  );
}

function Text({ x, text, color }: { x: number; text: string; color: number }) {
  return (
    <div
      data-testid="guild-text"
      style={{
        position: 'absolute',
        left: x,
        top: 0,
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
  row: GuildMemberRow;
  y: number;
  width: number;
  selected: boolean;
}) {
  const color = selected
    ? TEXT_COLOR.selected
    : row.online
      ? TEXT_COLOR.online
      : TEXT_COLOR.offline;
  const tint = !row.online;
  const marks = memberJobMarks(row.jobType, row.job, row.level);
  return (
    <div
      data-testid="guild-row"
      data-id={row.id}
      data-selected={selected}
      data-online={row.online}
      style={{ position: 'absolute', left: 0, top: y, width, height: ROW_PITCH }}
    >
      {rankMarks(row.rank, row.cls).map((m, i) => (
        // the stars of the original are drawn untinted (`0xffffffff`)
        <AtlasIcon key={i} env={env} sprite={m.sprite} x={m.x} y={RANK_DY} tint={false} />
      ))}
      <AtlasIcon env={env} sprite={marks.type} x={COL.jobType} y={JOB_DY} tint={tint} />
      <AtlasIcon env={env} sprite={marks.job} x={COL.job} y={JOB_DY} tint={tint} />
      <Text x={COL.level} text={String(row.level)} color={color} />
      <Text x={COL.name} text={cutMemberName(row.name)} color={color} />
      <Text x={COL.alias} text={row.alias} color={color} />
    </div>
  );
});

export function GuildMemberListView({ control }: { control: GuildMemberList }) {
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
        key={row.id}
        env={control.env}
        row={row}
        y={ROWS_TOP + (i - s.scroll) * ROW_PITCH}
        width={rectWidth(c)}
        selected={s.selectedId === row.id}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="guild-member-list"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      <svg width="0" height="0" style={{ position: 'absolute' }} aria-hidden="true">
        <filter id={OFFLINE_FILTER_ID} colorInterpolationFilters="sRGB">
          <feColorMatrix
            type="matrix"
            values="1 0 0 0 0  0 0.392 0 0 0  0 0 0.392 0 0  0 0 0 1 0"
          />
        </filter>
      </svg>
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
