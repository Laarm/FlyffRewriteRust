// Communication -> System mail (Plan/25 section 8.2): the grant panel in Mail mode (recipients as in 5.3, a subject, a text, attachments from the
// basket and penya) with the history of the system mails below it: when, to whom, the subject, what is still attached and whether the player
// read it. One mail per item line is sent by the system ("[System]"), also to characters that are offline.
import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import {
  Badge,
  Button,
  ErrorState,
  Field,
  NumberInput,
  Skeleton,
  Table,
  type Column,
} from '../../components';
import { useNow } from '../../app/polling';
import type { SystemMailDto } from '../../gen/dto';
import {
  createGrantStore,
  ensureGrantData,
  GrantPanel,
  GrantProvider,
  useGrantData,
} from '../../grant';
import { formatAgo, formatDateTime } from '../dashboard/format';
import { useSnapshot } from '../dashboard/useSnapshot';
import { systemMails } from './api';
import { T } from './strings';

const M = T.mail;

/** The basket, recipients and text live here so that switching tabs and back keeps them. */
const tabStore = createGrantStore({ mode: 'mail' });

/** Tests: a fresh basket. */
export function resetMailTab(): void {
  tabStore.setState(createGrantStore({ mode: 'mail' }).getState());
}

export function attachmentText(
  m: Pick<SystemMailDto, 'item_id' | 'item_count' | 'gold'>,
  itemName: (id: number) => string,
): string {
  const parts: string[] = [];
  if (m.item_id !== 0) parts.push(M.items(m.item_count, itemName(m.item_id)));
  if (m.gold > 0) parts.push(M.penya(m.gold));
  return parts.length === 0 ? M.nothing : parts.join(' + ');
}

function History() {
  const [character, setCharacter] = useState<number | null>(null);
  const data = useGrantData((s) => s.data);
  const report = useStore(tabStore, (s) => s.report);
  const snap = useSnapshot(
    (signal) => systemMails(200, character, signal),
    `mail|${String(character)}`,
    30_000,
  );
  const nowMs = useNow(30_000);
  // a grant that was just sent shows up at once
  const { reload } = snap;
  useEffect(() => {
    if (report !== null) reload();
  }, [report, reload]);
  useEffect(() => {
    void ensureGrantData();
  }, []);
  const itemName = (id: number): string => {
    const n = data?.itemName(id);
    return n === undefined || n === '' ? `#${String(id)}` : n;
  };
  const nowS = Math.floor(nowMs / 1000);

  const columns: Column<SystemMailDto>[] = [
    {
      id: 'sent',
      header: M.sent,
      cell: (m) => (
        <span title={formatDateTime(m.created_at)}>{formatAgo(m.created_at, nowS)}</span>
      ),
      text: (m) => m.created_at,
      sortable: true,
      sortValue: (m) => m.created_at,
      width: '110px',
    },
    {
      id: 'to',
      header: M.to,
      cell: (m) => (
        <span>
          {m.receiver_name} {m.online && <Badge tone="ok">{M.online}</Badge>}
        </span>
      ),
      text: (m) => m.receiver_name,
      filterable: true,
      width: '170px',
    },
    {
      id: 'subject',
      header: M.subject,
      cell: (m) => <span title={m.text}>{m.title}</span>,
      text: (m) => m.title,
      filterable: true,
    },
    {
      id: 'attachment',
      header: M.attachment,
      cell: (m) => attachmentText(m, itemName),
      text: (m) => attachmentText(m, itemName),
      filterable: true,
    },
    {
      id: 'state',
      header: M.state,
      cell: (m) => <Badge tone={m.read ? 'neutral' : 'accent'}>{m.read ? M.read : M.unread}</Badge>,
      text: (m) => (m.read ? M.read : M.unread),
      width: '90px',
    },
  ];

  return (
    <section aria-label={M.historyTitle}>
      <header className="comm-head">
        <div>
          <h3 className="comm-h">{M.historyTitle}</h3>
          <p className="adm-muted">{M.historyHint}</p>
        </div>
        <div className="comm-filter">
          <Field label={M.character} hint={M.filterHint}>
            <NumberInput
              min={1}
              value={character ?? ''}
              onChange={(e) => {
                const n = Math.trunc(Number(e.target.value));
                setCharacter(e.target.value === '' || !Number.isFinite(n) || n < 1 ? null : n);
              }}
            />
          </Field>
          <Button icon="refresh" onClick={snap.reload}>
            {M.refresh}
          </Button>
        </div>
      </header>
      {snap.error !== null && snap.data === null ? (
        <ErrorState error={snap.error} onRetry={snap.reload} />
      ) : snap.data === null ? (
        <Skeleton lines={4} />
      ) : (
        <Table
          label={M.historyTitle}
          columns={columns}
          rows={snap.data.items}
          rowKey={(m) => String(m.id)}
          emptyTitle={M.empty}
          emptyHint={M.emptyHint}
          exportName="system-mail"
        />
      )}
    </section>
  );
}

export function MailTab() {
  return (
    <div className="comm-tab">
      <p className="adm-muted comm-intro">{M.intro}</p>
      <GrantProvider value={tabStore}>
        <GrantPanel variant="tab" lockMode="mail" />
      </GrantProvider>
      <History />
    </div>
  );
}
