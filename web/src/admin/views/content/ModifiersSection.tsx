// World & Content -> Rates & events -> Event.lua modifiers (Plan/25 section 7.1): the factors and bonuses the running events of Event.lua give
// (EXP, drops, penya, attack and defense, shop prices), read only, each with its source. The shipped repack's numbers are EXP x20, item drop
// x10, piece drop x10, penya x250, +1 attack and defense. Also: the Lua events that are running.
import { Badge, ErrorState, Skeleton, Table, type Column } from '../../components';
import type { EventInfoDto, ModifierRow } from '../../gen/dto';
import { T } from './strings';

const M = T.modifiers;

export function ModifiersSection({
  info,
  error,
  onRetry,
}: {
  readonly info: EventInfoDto | null;
  readonly error: unknown;
  readonly onRetry: () => void;
}) {
  const columns: Column<ModifierRow>[] = [
    {
      id: 'label',
      header: M.label,
      cell: (m) => (m.active ? <strong>{m.label}</strong> : m.label),
      text: (m) => m.label,
      filterable: true,
    },
    {
      id: 'value',
      header: M.value,
      cell: (m) => (
        <span>
          <span className={m.active ? 'content-effective' : 'adm-muted'}>
            {M.format(m.value, m.unit)}
          </span>{' '}
          {m.active && <Badge tone="accent">{M.active}</Badge>}
        </span>
      ),
      text: (m) => m.value,
      align: 'right',
      width: '170px',
    },
    {
      id: 'source',
      header: M.source,
      cell: (m) => <code className="adm-muted">{m.source}</code>,
      text: (m) => m.source,
    },
  ];
  return (
    <section className="content-section" aria-label={M.title}>
      <h3 className="content-h">{M.title}</h3>
      <p className="adm-muted">{M.hint}</p>
      {error !== null && info === null ? (
        <ErrorState error={error} onRetry={onRetry} />
      ) : info === null ? (
        <Skeleton lines={6} />
      ) : (
        <>
          <Table label={M.title} columns={columns} rows={info.modifiers} rowKey={(m) => m.key} />
          <h4 className="content-h4">{M.running}</h4>
          {info.running.length === 0 ? (
            <p className="adm-muted">{M.noneRunning}</p>
          ) : (
            <ul className="content-running">
              {info.running.map((r) => (
                <li key={r.id}>
                  <Badge tone="ok">#{r.id}</Badge> {r.desc}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}
