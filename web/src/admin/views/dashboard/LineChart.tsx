// A small inline SVG line chart (Plan/25 section 9.2; no chart library): up to three series over time, a 0-based y axis with nice
// ticks, hover values as text, and a summary as the accessible name. Series differ by line style as well as colour.
import { useId, useMemo, useState, type PointerEvent } from 'react';
import { linePath, nearestIndex, summarize, timeAt, timeTicks, yScale, type Box } from './charts';
import { formatClock } from './format';

export interface ChartSeries {
  readonly id: string;
  readonly label: string;
  readonly values: readonly number[];
}

export interface LineChartProps {
  readonly title: string;
  /** Unix seconds of each sample (ascending). */
  readonly t: readonly number[];
  readonly series: readonly ChartSeries[];
  /** Formats a y value (axis ticks and readout). */
  readonly format: (v: number) => string;
  /** The axis never ends below this (so an idle server does not show noise as a mountain). */
  readonly minMax?: number;
  readonly height?: number;
}

const WIDTH = 480;
const MARGIN = { left: 54, right: 10, top: 8, bottom: 22 };

export function LineChart({ title, t, series, format, minMax = 1, height = 150 }: LineChartProps) {
  const titleId = useId();
  const [hover, setHover] = useState<number | null>(null);
  const box: Box = {
    left: MARGIN.left,
    top: MARGIN.top,
    width: WIDTH - MARGIN.left - MARGIN.right,
    height: height - MARGIN.top - MARGIN.bottom,
  };
  const t0 = t[0] ?? 0;
  const t1 = t[t.length - 1] ?? t0 + 1;
  const dataMax = useMemo(
    () => Math.max(0, ...series.flatMap((s) => s.values.map((v) => (Number.isFinite(v) ? v : 0)))),
    [series],
  );
  const y = yScale(dataMax, minMax);
  const summary = series
    .map((s) => {
      const sum = summarize(s.values);
      return sum
        ? `${s.label}: now ${format(sum.last)}, peak ${format(sum.max)}, average ${format(sum.avg)}`
        : `${s.label}: no data`;
    })
    .join('; ');

  const onMove = (event: PointerEvent<SVGSVGElement>): void => {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / (rect.width || 1)) * WIDTH;
    setHover(nearestIndex(t, timeAt(x, box, t0, t1)));
  };
  const at = hover !== null && hover >= 0 && hover < t.length ? hover : null;
  const hoverT = at === null ? null : (t[at] ?? null);
  const hoverX = hoverT === null ? null : box.left + ((hoverT - t0) / (t1 - t0 || 1)) * box.width;

  return (
    <figure className="adm-chart" aria-labelledby={titleId}>
      <figcaption id={titleId} className="adm-chart__title">
        {title}
      </figcaption>
      {t.length < 2 ? (
        <p className="adm-muted adm-chart__empty">Collecting samples...</p>
      ) : (
        <svg
          className="adm-chart__svg"
          viewBox={`0 0 ${String(WIDTH)} ${String(height)}`}
          role="img"
          aria-label={`${title}. ${summary}`}
          onPointerMove={onMove}
          onPointerLeave={() => {
            setHover(null);
          }}
        >
          {y.ticks.map((v) => {
            const yy = box.top + box.height - (v / y.max) * box.height;
            return (
              <g key={v}>
                <line
                  className="adm-chart__grid"
                  x1={box.left}
                  x2={box.left + box.width}
                  y1={yy}
                  y2={yy}
                />
                <text className="adm-chart__tick" x={box.left - 6} y={yy + 4} textAnchor="end">
                  {format(v)}
                </text>
              </g>
            );
          })}
          {timeTicks(t0, t1, 5).map((tt) => {
            const x = box.left + ((tt - t0) / (t1 - t0 || 1)) * box.width;
            return (
              <text
                key={tt}
                className="adm-chart__tick"
                x={x}
                y={height - 6}
                textAnchor={
                  x <= box.left + 4 ? 'start' : x >= box.left + box.width - 4 ? 'end' : 'middle'
                }
              >
                {formatClock(tt)}
              </text>
            );
          })}
          {series.map((s, i) => (
            <path
              key={s.id}
              className={`adm-chart__line adm-chart__line--${String(i + 1)}`}
              d={linePath(t, s.values, box, t0, t1, y.max)}
              fill="none"
            />
          ))}
          {hoverX !== null && (
            <line
              className="adm-chart__cursor"
              x1={hoverX}
              x2={hoverX}
              y1={box.top}
              y2={box.top + box.height}
            />
          )}
        </svg>
      )}
      <ul className="adm-chart__legend">
        {series.map((s, i) => {
          const value = at === null ? s.values[s.values.length - 1] : s.values[at];
          return (
            <li key={s.id}>
              <span
                className={`adm-chart__key adm-chart__key--${String(i + 1)}`}
                aria-hidden="true"
              />
              {s.label}
              {value === undefined ? '' : `: ${format(value)}`}
            </li>
          );
        })}
        {hoverT !== null && <li className="adm-muted">{formatClock(hoverT)}</li>}
      </ul>
    </figure>
  );
}
