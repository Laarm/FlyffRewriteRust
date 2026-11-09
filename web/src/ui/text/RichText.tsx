// Renders laid-out rich text (Plan/42 §8.2-§8.3; `C2DRender::TextOut_EditString`, 2DRender.cpp:491-585): one block
// per line (`white-space: pre`, the line breaker already decided where lines end), one span per run. Bold is the
// glyph drawn twice at x and x + 1 (a 1 px `text-shadow` in the text colour), underline and strike-through are 1 px
// lines at `cell - 2` and `cell / 2` below the line top, drawn in the text colour.

import { memo, type CSSProperties } from 'react';
import { FONT, type FontSpec, fontStyle, lineHeight, outlineShadow } from './fonts';
import type { RichLine } from './layout';
import { cssColor, type RichRun } from './markup';

export interface RichTextProps {
  lines: readonly RichLine[];
  /** First line to render (the scroll position), default 0. */
  first?: number;
  /** Number of lines to render (default: all from `first`). */
  count?: number;
  font?: FontSpec;
  /** Extra pixels below every line (`m_nLineSpace`: Text 5, ListBox 1, chat 2, tooltip 2). */
  lineSpace?: number;
  className?: string;
  style?: CSSProperties;
}

/** The CSS of one run of a font whose line cell is `cell` px. */
export function runStyle(run: RichRun, cell: number, outline = ''): CSSProperties {
  const style: CSSProperties = { color: cssColor(run.color) };
  if (run.bold)
    style.textShadow = outline === '' ? '1px 0 0 currentColor' : `${outline}, 1px 0 0 currentColor`;
  const layers: string[] = [];
  const positions: string[] = [];
  const sizes: string[] = [];
  const line = 'linear-gradient(currentColor, currentColor)';
  if (run.underline) {
    layers.push(line);
    positions.push(`0 ${String(cell - 2)}px`);
    sizes.push('100% 1px');
  }
  if (run.strike) {
    layers.push(line);
    positions.push(`0 ${String(Math.floor(cell / 2))}px`);
    sizes.push('100% 1px');
  }
  if (layers.length > 0) {
    style.backgroundImage = layers.join(', ');
    style.backgroundPosition = positions.join(', ');
    style.backgroundSize = sizes.join(', ');
    style.backgroundRepeat = 'no-repeat';
  }
  return style;
}

const Run = memo(function Run({
  run,
  cell,
  outline,
}: {
  run: RichRun;
  cell: number;
  outline: string;
}) {
  const decorated = run.bold || run.underline || run.strike;
  return (
    <span
      className={decorated ? 'rt-run rt-styled' : 'rt-run'}
      style={runStyle(run, cell, outline)}
    >
      {run.text}
    </span>
  );
});

export const RichText = memo(function RichText({
  lines,
  first = 0,
  count,
  font = FONT.text,
  lineSpace = 0,
  className,
  style,
}: RichTextProps) {
  const cell = lineHeight(font);
  const outline =
    font.outlinePx > 0 && font.outlineColor ? outlineShadow(font.outlinePx, font.outlineColor) : '';
  const end = count === undefined ? lines.length : Math.min(lines.length, first + count);
  const rows = [];
  for (let i = Math.max(0, first); i < end; i += 1) {
    const line = lines[i];
    if (!line) continue;
    rows.push(
      <div
        key={i}
        className="rt-line"
        style={{ height: cell + lineSpace, lineHeight: `${String(cell)}px` }}
      >
        {line.runs.map((run, ri) => (
          <Run key={ri} run={run} cell={cell} outline={outline} />
        ))}
      </div>,
    );
  }
  return (
    <div
      className={className === undefined ? 'rt' : `rt ${className}`}
      style={{ ...fontStyle(font), ...style }}
    >
      {rows}
    </div>
  );
});
