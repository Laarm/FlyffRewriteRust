// A small spinner for loading states of a control; whole panels use Skeleton.
export function Spinner({ label = 'Loading' }: { readonly label?: string }) {
  return (
    <span role="status" className="adm-spinner-wrap">
      <span className="adm-spinner" aria-hidden="true" />
      <span className="adm-sr">{label}</span>
    </span>
  );
}

/** Placeholder lines while the first data of a panel loads. */
export function Skeleton({
  lines = 3,
  height = 14,
}: {
  readonly lines?: number;
  readonly height?: number;
}) {
  return (
    <div className="adm-skeleton-stack" role="status" aria-label="Loading">
      {Array.from({ length: lines }, (_, i) => (
        <div
          key={i}
          className="adm-skeleton"
          style={{ height, width: `${String(100 - ((i * 17) % 40))}%` }}
        />
      ))}
    </div>
  );
}
