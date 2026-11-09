// Communication -> Notices (Plan/25 section 8.1): send a notice now (everybody, one world or some characters, as the /sys line or as a
// caption) and keep the scheduled and recurring notices (create, edit, pause, delete) with their next and last time. The schedule runs on
// the game thread (the wall job `admin.notices`); this page edits the rows. Times are server time.
import { useEffect, useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import { useNow } from '../../app/polling';
import { Badge, Button, ErrorState, Skeleton, Table, type Column } from '../../components';
import { loadWorlds } from '../../components/mapMeta';
import type { NoticeList, NoticeRow } from '../../gen/dto';
import { openDialog } from '../../stores/dialogs';
import { useCan } from '../../stores/session';
import { toast } from '../../stores/toasts';
import type { WorldsDoc } from '../../world/metaTypes';
import { useSnapshot } from '../dashboard/useSnapshot';
import { bodyOf, broadcast, deleteNotice, listNotices, updateNotice } from './api';
import {
  DEFAULT_LIMITS,
  type Draft,
  EMPTY_DRAFT,
  type Limits,
  MAX_CHARACTERS,
  sendable,
  targetOf,
} from './draft';
import { NoticeFields, worldLabel } from './NoticeFields';
import { describeRepeat, describeTarget, stateTone } from './repeat';
import { ScheduleDialog } from './ScheduleDialog';
import { T } from './strings';
import { formatOffset, formatUntil, formatZoned } from './zone';

const N = T.notices;

function limitsOf(list: NoticeList | null): Limits {
  return list ? { system: list.max_system_bytes, caption: list.max_caption_bytes } : DEFAULT_LIMITS;
}

function SendNow({ limits }: { readonly limits: Limits }) {
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [busy, setBusy] = useState(false);
  const [worlds, setWorlds] = useState<WorldsDoc | null>(null);
  useEffect(() => {
    let live = true;
    loadWorlds().then(
      (doc) => {
        if (live) setWorlds(doc);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  const target = targetOf(draft.target);
  const ready = sendable(draft, limits);

  const who = (): string => {
    switch (draft.target.kind) {
      case 'all':
        return N.whoAll;
      case 'world':
        return N.whoWorld(worldLabel(worlds, draft.target.world ?? 0));
      case 'characters':
        return N.whoCharacters(draft.target.characters.length);
    }
  };

  const send = (): void => {
    if (target === null || !ready) return;
    setBusy(true);
    const count = draft.target.kind === 'characters' ? draft.target.characters.length : 1;
    void runConfirmed({
      title: N.sendTitle,
      message: (
        <div className="comm-confirm">
          <p>{N.sendBody(who())}</p>
          <blockquote className={`comm-preview comm-preview--${draft.style}`}>
            {draft.text.trim()}
          </blockquote>
        </div>
      ),
      confirmLabel: N.send,
      reason: 'optional',
      facts: { targets: count },
      run: (given) =>
        broadcast(
          { text: draft.text.trim(), style: draft.style, target },
          given.reason,
          given.confirm,
        ),
      success: (r) =>
        r.requested !== null && r.delivered < r.requested
          ? N.sentSome(r.delivered, r.requested)
          : N.sentAll(r.delivered),
    }).then((o) => {
      setBusy(false);
      if (o.ok) setDraft({ ...draft, text: '' });
    });
  };

  return (
    <section className="comm-card" aria-label={N.nowTitle}>
      <h3 className="comm-h">{N.nowTitle}</h3>
      <p className="adm-muted">{N.nowHint}</p>
      <NoticeFields draft={draft} onChange={setDraft} limits={limits} />
      {draft.target.kind === 'characters' && draft.target.characters.length > MAX_CHARACTERS && (
        <div className="adm-field__error">{N.tooMany(MAX_CHARACTERS)}</div>
      )}
      <div className="comm-actions">
        <Button variant="primary" icon="message" loading={busy} disabled={!ready} onClick={send}>
          {N.send}
        </Button>
      </div>
    </section>
  );
}

export function NoticesTab() {
  const canEdit = useCan('act.broadcast');
  // the clock of the page is the server's: the list tells its time and the page counts on from the moment the list arrived
  const snap = useSnapshot(
    (signal) => listNotices(signal).then((l) => ({ list: l, received: Date.now() })),
    'notices',
    15_000,
  );
  const nowMs = useNow(15_000);
  const [worlds, setWorlds] = useState<WorldsDoc | null>(null);
  useEffect(() => {
    let live = true;
    loadWorlds().then(
      (doc) => {
        if (live) setWorlds(doc);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  const list = snap.data?.list ?? null;
  const limits = limitsOf(list);
  const zone = list?.zone ?? 'UTC';
  const nowS = list
    ? list.now + Math.max(0, Math.floor((nowMs - (snap.data?.received ?? nowMs)) / 1000))
    : Math.floor(nowMs / 1000);

  const open = (row: NoticeRow | null): void => {
    if (!list) return;
    openDialog((close) => (
      <ScheduleDialog
        row={row}
        zone={list.zone}
        limits={limits}
        nowS={list.now}
        onSaved={snap.reload}
        close={close}
      />
    ));
  };

  const togglePause = (row: NoticeRow): void => {
    updateNotice(row.id, { ...bodyOf(row), paused: !row.paused }).then(
      () => {
        toast.success(row.paused ? N.resumed : N.paused_);
        snap.reload();
      },
      (error: unknown) => {
        toast.error(error, row.paused ? N.resume : N.pause);
      },
    );
  };

  const remove = (row: NoticeRow): void => {
    void runConfirmed({
      title: N.deleteTitle(row.id),
      message: (
        <div className="comm-confirm">
          <p>{N.deleteBody}</p>
          <blockquote className="comm-preview">{row.text}</blockquote>
        </div>
      ),
      confirmLabel: N.delete,
      danger: true,
      reason: 'optional',
      run: (given) => deleteNotice(row.id, given.reason),
      success: N.deleted,
    }).then((o) => {
      if (o.ok) snap.reload();
    });
  };

  const columns: Column<NoticeRow>[] = (() => {
    const cols: Column<NoticeRow>[] = [
      {
        id: 'id',
        header: 'ID',
        cell: (r) => r.id,
        text: (r) => r.id,
        align: 'right',
        width: '56px',
      },
      {
        id: 'text',
        header: N.colText,
        cell: (r) => (
          <span className="comm-text" title={r.text}>
            <Badge>{N.styleShort[r.style]}</Badge> {r.text}
          </span>
        ),
        text: (r) => r.text,
        filterable: true,
      },
      {
        id: 'target',
        header: N.colTarget,
        cell: (r) => describeTarget(r.target, (id) => worldLabel(worlds, id)),
        text: (r) => describeTarget(r.target, (id) => worldLabel(worlds, id)),
        width: '150px',
      },
      {
        id: 'when',
        header: N.colWhen,
        cell: (r) => (
          <span>
            {formatZoned(r.start_at, zone)}
            <br />
            <span className="adm-muted">
              {describeRepeat(r.repeat)}
              {r.end_at === null ? '' : `, ${N.until} ${formatZoned(r.end_at, zone)}`}
            </span>
          </span>
        ),
        text: (r) => `${formatZoned(r.start_at, zone)} ${describeRepeat(r.repeat)}`,
        width: '210px',
      },
      {
        id: 'next',
        header: N.colNext,
        cell: (r) =>
          r.next_fire === null ? (
            <span className="adm-muted">-</span>
          ) : (
            <span title={formatZoned(r.next_fire, zone)}>{formatUntil(r.next_fire, nowS)}</span>
          ),
        text: (r) => r.next_fire ?? 0,
        sortable: true,
        sortValue: (r) => r.next_fire ?? Number.MAX_SAFE_INTEGER,
        width: '110px',
      },
      {
        id: 'last',
        header: N.colLast,
        cell: (r) =>
          r.last_fired_at === null ? (
            <span className="adm-muted">{N.never}</span>
          ) : (
            formatZoned(r.last_fired_at, zone)
          ),
        text: (r) => r.last_fired_at ?? 0,
        width: '130px',
      },
      {
        id: 'state',
        header: N.colState,
        cell: (r) => (
          <Badge tone={stateTone(r.state)} title={stateHint(r.state)}>
            {stateLabel(r.state)}
          </Badge>
        ),
        text: (r) => r.state,
        filterable: true,
        width: '100px',
      },
    ];
    if (canEdit) {
      cols.push({
        id: 'actions',
        header: '',
        cell: (r) => (
          <span className="comm-rowactions">
            <Button
              size="sm"
              aria-label={`${r.paused ? N.resume : N.pause} #${String(r.id)}`}
              onClick={() => {
                togglePause(r);
              }}
            >
              {r.paused ? N.resume : N.pause}
            </Button>
            <Button
              size="sm"
              aria-label={`${N.edit} #${String(r.id)}`}
              onClick={() => {
                open(r);
              }}
            >
              {N.edit}
            </Button>
            <Button
              size="sm"
              variant="danger"
              aria-label={`${N.delete} #${String(r.id)}`}
              onClick={() => {
                remove(r);
              }}
            >
              {N.delete}
            </Button>
          </span>
        ),
        width: '250px',
      });
    }
    return cols;
  })();

  return (
    <div className="comm-tab">
      <SendNow limits={limits} />
      <section aria-label={N.scheduleTitle}>
        <header className="comm-head">
          <div>
            <h3 className="comm-h">{N.scheduleTitle}</h3>
            {list && (
              <p className="adm-muted">
                {N.scheduleHint(list.zone, formatOffset(list.utc_offset_s))}
              </p>
            )}
          </div>
          {canEdit && (
            <Button
              variant="primary"
              disabled={!list}
              onClick={() => {
                open(null);
              }}
            >
              {N.new}
            </Button>
          )}
        </header>
        {snap.error !== null && list === null ? (
          <ErrorState error={snap.error} onRetry={snap.reload} />
        ) : list === null ? (
          <Skeleton lines={4} />
        ) : (
          <Table
            label={N.scheduleTitle}
            columns={columns}
            rows={list.items}
            rowKey={(r) => String(r.id)}
            emptyTitle={N.empty}
            emptyHint={N.emptyHint}
          />
        )}
      </section>
    </div>
  );
}

function stateLabel(state: string): string {
  switch (state) {
    case 'scheduled':
    case 'paused':
    case 'done':
    case 'missed':
    case 'ended':
      return N.states[state];
    default:
      return state;
  }
}

function stateHint(state: string): string {
  switch (state) {
    case 'scheduled':
    case 'paused':
    case 'done':
    case 'missed':
    case 'ended':
      return N.stateHints[state];
    default:
      return state;
  }
}
