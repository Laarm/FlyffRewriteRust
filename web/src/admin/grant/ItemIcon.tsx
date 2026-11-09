// The picture of an item: `item/<szIcon>` through the generated PNG URL. 19 propItem icons are missing from `Resource/Item`: those items
// (and every icon that fails to load) show a "?" cell, with the reason as its title (Plan/25 section 5.1).
import { useState } from 'react';
import { useGrantData } from './data';
import { T } from './strings';

export function ItemIcon({
  icon,
  size = 48,
  label,
}: {
  readonly icon: string;
  readonly size?: number;
  readonly label?: string;
}) {
  const data = useGrantData((s) => s.data);
  const [failed, setFailed] = useState<string | null>(null);
  const url = data?.iconUrl(icon) ?? null;
  const style = { width: size, height: size };
  if (url === null || failed === url) {
    return (
      <span
        className="grant-icon grant-icon--missing"
        style={style}
        title={T.icon.missing}
        role="img"
        aria-label={label ?? T.icon.missing}
      >
        ?
      </span>
    );
  }
  return (
    <img
      className="grant-icon"
      src={url}
      style={style}
      alt={label ?? ''}
      loading="lazy"
      draggable={false}
      onError={() => {
        setFailed(url);
      }}
    />
  );
}
