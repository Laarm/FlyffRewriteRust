// Players -> Characters (Plan/25 section 4.2): search by name prefix, id or account across online, offline and deleted characters
// (`GET /admin/api/characters`, chunked on the server, merged with the live roster), with status chips. A click opens the drawer.
import { useState } from 'react';
import type { TabProps } from '../../app/views';
import {
  Badge,
  Field,
  Select,
  Table,
  TextInput,
  usePagedList,
  type Column,
} from '../../components';
import type { CharacterListItem } from '../../gen/dto';
import { useNow } from '../../app/polling';
import { blockedText } from './characterActions';
import { formatAgo, formatDateTime } from './format';
import { useDebounced } from './hooks';
import { JobIcon } from './JobIcon';
import { jobName } from './jobs';
import { openCharacter, RowMenu } from './rowMenu';
import { T } from './strings';

type StatusFilter = keyof typeof T.characters.statuses;

export function StatusChips({ c }: { readonly c: CharacterListItem }) {
  const now = Math.floor(useNow(30_000) / 1000);
  return (
    <span className="adm-row">
      {c.online ? (
        <Badge tone="ok">{T.common.online}</Badge>
      ) : (
        <Badge>{`${T.common.offline}, ${formatAgo(now, c.last_seen)}`}</Badge>
      )}
      {c.status === 'deleted' ? (
        <Badge tone="danger">
          {c.purge_in_days === null ? T.common.deleted : T.characters.purgeIn(c.purge_in_days)}
        </Badge>
      ) : null}
      {c.status === 'sealed' ? <Badge tone="warn">{T.common.sealed}</Badge> : null}
      {c.login_blocked_until !== null ? (
        <Badge tone="warn" title={blockedText(c.login_blocked_until)}>
          {T.common.blocked}
        </Badge>
      ) : null}
    </span>
  );
}

export function CharactersTab({ route }: TabProps) {
  const [q, setQ] = useState('');
  const [status, setStatus] = useState<StatusFilter>('any');
  const text = useDebounced(q.trim());
  const list = usePagedList<CharacterListItem>({
    path: '/admin/api/characters',
    query: { ...(text === '' ? {} : { q: text }), ...(status === 'any' ? {} : { status }) },
    limit: 50,
  });
  const columns: Column<CharacterListItem>[] = [
    {
      id: 'name',
      header: T.characters.columns.name,
      sortable: true,
      cell: (c) => (
        <span className="adm-row">
          <JobIcon job={c.job} size={20} />
          <strong>{c.name}</strong>
        </span>
      ),
      text: (c) => c.name,
    },
    {
      id: 'account',
      header: T.characters.columns.account,
      sortable: true,
      cell: (c) => c.account,
      text: (c) => c.account,
    },
    {
      id: 'level',
      header: T.characters.columns.level,
      sortable: true,
      align: 'right',
      cell: (c) => c.level,
      text: (c) => c.level,
    },
    {
      id: 'job',
      header: T.characters.columns.job,
      sortable: true,
      cell: (c) => jobName(c.job),
      text: (c) => jobName(c.job),
    },
    {
      id: 'state',
      header: T.characters.columns.state,
      cell: (c) => <StatusChips c={c} />,
      text: (c) => (c.online ? 'online' : c.status),
    },
    {
      id: 'seen',
      header: T.characters.columns.lastSeen,
      sortable: true,
      cell: (c) => formatDateTime(c.last_seen),
      text: (c) => c.last_seen,
    },
    {
      id: 'created',
      header: T.characters.columns.created,
      sortable: true,
      cell: (c) => formatDateTime(c.created_at),
      text: (c) => c.created_at,
    },
    {
      id: 'actions',
      header: '',
      width: '48px',
      cell: (c) => (
        <RowMenu
          target={{
            id: c.id,
            name: c.name,
            account: c.account,
            online: c.online,
            world: c.world,
            deleted: c.status === 'deleted',
          }}
        />
      ),
    },
  ];
  return (
    <div className="plr-tab" data-testid="characters-tab">
      <div className="plr-bar-top">
        <Field label={T.characters.searchLabel} hint={T.characters.searchHint}>
          <TextInput
            type="search"
            value={q}
            autoFocus
            onChange={(e) => {
              setQ(e.target.value);
            }}
          />
        </Field>
        <Field label={T.characters.status}>
          <Select
            value={status}
            onChange={(e) => {
              setStatus(e.target.value as StatusFilter);
            }}
          >
            {(Object.keys(T.characters.statuses) as StatusFilter[]).map((s) => (
              <option key={s} value={s}>
                {T.characters.statuses[s]}
              </option>
            ))}
          </Select>
        </Field>
      </div>
      <Table
        label="Characters"
        columns={columns}
        rows={list.rows}
        rowKey={(c) => String(c.id)}
        loading={list.loading}
        error={list.error}
        onRetry={list.reload}
        hasMore={list.hasMore}
        loadingMore={list.loadingMore}
        onLoadMore={list.loadMore}
        emptyTitle={T.characters.empty}
        emptyHint={T.characters.emptyHint}
        onRowActivate={(c) => {
          openCharacter(c.id);
        }}
        selectedKey={route.params['c'] ?? null}
        exportName="characters"
      />
    </div>
  );
}
