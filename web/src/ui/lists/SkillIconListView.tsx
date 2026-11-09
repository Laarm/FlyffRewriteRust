// The rows of the skill list (`CWndSkillCtrl::OnDraw`): icon, name, `Lv%2d`, the `Master` mark or the level gauge.

import { memo, useSyncExternalStore } from 'react';
import { useSprite } from '../controls/useSprite';
import { iconImageStyle } from '../icons/IconCell';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, fontStyle, lineHeight, textWidth } from '../text/fonts';
import { cssColor } from '../text/markup';
import type { WndEnv } from '../wnd/env';
import { ControlHost } from '../wnd/FrameWindowView';
import { PaintedFrame } from './SlotGridView';
import { visibleRows } from './geometry';
import { type SkillIconList, type SkillRow, SKILL_ROW_PITCH } from './SkillIconList';

const TEXT = 0xff000000;
const SELECTED = 0xff6060ff;

/** `sprintf("Lv%2d")`. */
export const levelText = (level: number): string => `Lv${String(level).padStart(2, ' ')}`;

const Row = memo(function Row({
  env,
  row,
  y,
  width,
  selected,
  hover,
}: {
  env: WndEnv;
  row: SkillRow;
  y: number;
  width: number;
  selected: boolean;
  hover: boolean;
}) {
  const sprite = useSprite(env, row.icon);
  const color = cssColor(selected ? SELECTED : TEXT);
  const master = row.level >= row.maxLevel;
  const lv = levelText(row.level);
  const lvW = textWidth(FONT.text, lv);
  return (
    <div
      data-testid="skill-row"
      data-skill={row.id}
      data-selected={selected}
      data-enabled={row.enabled}
      data-hover={hover}
      style={{ position: 'absolute', left: 0, top: y, width, height: SKILL_ROW_PITCH }}
    >
      <div
        data-testid="skill-icon"
        style={iconImageStyle({
          url: typeof sprite === 'object' ? sprite.url : null,
          size: 32,
          left: 3,
          top: 6,
          alpha: row.enabled ? 1 : 64 / 255,
          tint: false,
        })}
      />
      <div
        data-testid="skill-name"
        style={{
          position: 'absolute',
          left: 65,
          top: 10,
          color,
          whiteSpace: 'pre',
          ...fontStyle(FONT.text),
        }}
      >
        {row.name}
      </div>
      <div
        data-testid="skill-level"
        style={{
          position: 'absolute',
          left: 58 - lvW,
          top: 6 + 30 - lineHeight(FONT.text),
          color,
          whiteSpace: 'pre',
          ...fontStyle(FONT.text),
        }}
      >
        {lv}
      </div>
      {master ? (
        <div
          data-testid="skill-master"
          style={{
            position: 'absolute',
            // `rect.left + 83 - size.cx + ax / 2` with the width of the LEVEL text (the C++ reuses `size`)
            left: 91 - lvW,
            top: 6 + 30 - lineHeight(FONT.text),
            color: '#ff0000',
            whiteSpace: 'pre',
            ...fontStyle(FONT.text),
          }}
        >
          Master
        </div>
      ) : (
        <div
          data-testid="skill-gauge"
          style={{
            position: 'absolute',
            left: 60,
            top: 25,
            width: Math.max(0, width - 70),
            height: 10,
            boxSizing: 'border-box',
            border: '1px solid #606060',
            background: '#d8d8d8',
          }}
        >
          <div
            data-testid="skill-gauge-fill"
            style={{
              width: `${String(Math.round(Math.max(0, Math.min(1, row.progress ?? 1)) * 100))}%`,
              height: '100%',
              background: 'rgba(255, 0, 0, 0.39)',
            }}
          />
        </div>
      )}
    </div>
  );
});

export function SkillIconListView({ control }: { control: SkillIconList }) {
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
        y={control.geometry.top + (i - s.scroll) * SKILL_ROW_PITCH}
        width={rectWidth(c)}
        selected={s.selected === i}
        hover={s.hover === i}
      />,
    );
  }
  const bar = control.scrollBar;
  return (
    <div
      className="ig"
      data-testid="skill-list"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      <PaintedFrame width={rectWidth(w)} height={rectHeight(w)} />
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
