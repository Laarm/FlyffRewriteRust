// Sparkline: a tiny inline SVG line chart for the dashboard cards (no chart library, Plan/25 section 9.2).
export interface SparklineProps {
  readonly values: readonly number[];
  readonly width?: number;
  readonly height?: number;
  /** Text for screen readers ("Tick p50, last hour"). */
  readonly label: string;
  /** The value the line is capped at on the top (default: the maximum of the data). */
  readonly max?: number;
}

/** The `points` attribute of the polyline: values scaled into `width x height` with 2 px of padding. */
export function sparklinePoints(
  values: readonly number[],
  width: number,
  height: number,
  max?: number,
): string {
  if (values.length === 0) return '';
  const hi = max ?? Math.max(...values, 0);
  const lo = Math.min(...values, 0);
  const span = hi - lo || 1;
  const pad = 2;
  const stepX = values.length > 1 ? (width - pad * 2) / (values.length - 1) : 0;
  return values
    .map((v, i) => {
      const x = pad + i * stepX;
      const y = height - pad - ((Math.min(v, hi) - lo) / span) * (height - pad * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');
}

export function Sparkline({ values, width = 120, height = 32, label, max }: SparklineProps) {
  return (
    <svg
      className="adm-sparkline"
      width={width}
      height={height}
      viewBox={`0 0 ${String(width)} ${String(height)}`}
      role="img"
      aria-label={label}
    >
      <polyline
        points={sparklinePoints(values, width, height, max)}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}
