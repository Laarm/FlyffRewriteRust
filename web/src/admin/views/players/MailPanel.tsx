// Items -> Mail (n/50) (Plan/25 section 4.4): the character's mailbox, live from the game thread (`Query::Mailbox`), each mail with its
// attachment (an item cell with the game tooltip), its penya and the time left before it is deleted.
import { useEffect, useState } from 'react';
import { Badge, ErrorState, Skeleton, Table, type Column } from '../../components';
import type { MailRow, MailboxView } from '../../gen/dto';
import { useCharacter } from './context';
import { decodeItem } from './containers';
import { fetchMail } from './api';
import { formatAgo, formatNumber, formatSpan } from './format';
import { ItemCell } from './ItemCell';
import { usePlayerGame } from './gamedata';
import { T } from './strings';
import { useNow } from '../../app/polling';

/** The mailbox of a character, read now and every 20 s. */
export function useMailbox(
  characterId: number,
  enabled: boolean,
): {
  readonly mailbox: MailboxView | null;
  readonly error: unknown;
  readonly reload: () => void;
} {
  const [mailbox, setMailbox] = useState<MailboxView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    const load = (): void => {
      fetchMail(characterId).then(
        (m) => {
          if (live) {
            setMailbox(m);
            setError(null);
          }
        },
        (e: unknown) => {
          if (live) setError(e);
        },
      );
    };
    load();
    const id = setInterval(load, 20_000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [characterId, enabled, round]);
  return {
    mailbox,
    error,
    reload: () => {
      setRound((n) => n + 1);
    },
  };
}

export function MailPanel({
  mailbox,
  error,
}: {
  readonly mailbox: MailboxView | null;
  readonly error: unknown;
}) {
  const { live } = useCharacter();
  const game = usePlayerGame();
  const now = Math.floor(useNow(1000) / 1000);
  if (error && !mailbox) return <ErrorState error={error} />;
  if (!mailbox) return <Skeleton lines={4} height={18} />;
  const columns: Column<MailRow>[] = [
    {
      id: 'from',
      header: T.items.mail.from,
      cell: (m) => (m.sender_id === 0 ? T.items.mail.system : m.sender_name),
      text: (m) => (m.sender_id === 0 ? T.items.mail.system : m.sender_name),
    },
    {
      id: 'title',
      header: T.items.mail.title,
      cell: (m) => (
        <span>
          <strong>{m.title}</strong>
          {m.text !== '' ? <div className="adm-muted adm-small">{m.text}</div> : null}
        </span>
      ),
      text: (m) => `${m.title} ${m.text}`,
    },
    {
      id: 'penya',
      header: T.items.mail.penya,
      cell: (m) => (m.gold > 0 ? formatNumber(m.gold) : '-'),
      text: (m) => m.gold,
      align: 'right',
    },
    {
      id: 'attachment',
      header: T.items.mail.attachment,
      cell: (m) => {
        const elem = m.item_pb ? decodeItem(m.item_pb) : null;
        return elem ? (
          <ItemCell elem={elem} slot={m.item?.slot ?? 0} game={game.data} />
        ) : (
          <span className="adm-muted">{T.items.mail.noAttachment}</span>
        );
      },
      text: (m) => (m.item ? String(m.item.item_id) : ''),
    },
    {
      id: 'expires',
      header: T.items.mail.expires,
      cell: (m) => formatSpan(Math.max(0, m.expires_at - now)),
      text: (m) => m.expires_at,
    },
    {
      id: 'sent',
      header: 'Sent',
      cell: (m) => formatAgo(now, m.created_at),
      text: (m) => m.created_at,
    },
    {
      id: 'read',
      header: '',
      cell: (m) =>
        m.read ? (
          <Badge>{T.items.mail.read}</Badge>
        ) : (
          <Badge tone="accent">{T.items.mail.unread}</Badge>
        ),
      text: (m) => (m.read ? T.items.mail.read : T.items.mail.unread),
    },
  ];
  return (
    <Table
      label="Mailbox"
      columns={columns}
      rows={mailbox.mails}
      rowKey={(m) => String(m.id)}
      loading={live.status === 'loading'}
      emptyTitle={T.items.mail.none}
    />
  );
}
