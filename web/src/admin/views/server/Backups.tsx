// Server -> Backups & maintenance (Plan/25 section 9.7; WP-ADMIN-26): the backup files with "Backup now", "Verify" and a download
// that only authority P gets, and only from the host itself unless `admin.allow_backup_download_remote` is on; the maintenance mode
// (phase-gated on WP-AUTH-18: the state is shown, the toggle says what is missing); shutdown with a countdown and message, cancel,
// and "Restart" only when a supervisor started the server (systemd `NOTIFY_SOCKET` / `INVOCATION_ID`). Restore stays a command
// line operation with the server stopped: the page shows the command.
import { useState } from 'react';
import { mutation, rest } from '../../api/rest';
import { runConfirmed } from '../../app/mutate';
import { authorityAtLeast } from '../../gen/caps';
import {
  Badge,
  Button,
  ErrorState,
  Field,
  NeedsFeature,
  NumberInput,
  Skeleton,
  Table,
  TextInput,
  type Column,
} from '../../components';
import { useCan, useSession } from '../../stores/session';
import { toast } from '../../stores/toasts';
import { formatAgo, formatBytes, formatDateTime } from '../dashboard/format';
import { useSnapshot } from '../dashboard/useSnapshot';
import { useNow } from '../../app/polling';
import type { BackupFile, BackupsResponse, MaintenanceResponse, Verified } from './api';
import { T } from './strings';

/** May this session download a backup? P only; a remote session only when the live key allows it. */
export function canDownload(
  authority: string,
  remote: boolean,
  remoteAllowed: boolean,
): { readonly ok: boolean; readonly why: string } {
  if (!authorityAtLeast(authority, 'P')) return { ok: false, why: T.backups.downloadNeedsP };
  if (remote && !remoteAllowed) return { ok: false, why: T.backups.downloadLocalOnly };
  return { ok: true, why: '' };
}

function Backups() {
  const canServer = useCan('act.server');
  const info = useSession((s) => s.info);
  const snap = useSnapshot(
    (signal) => rest.get<BackupsResponse>('/admin/api/backups', { signal }),
    'backups',
    15_000,
  );
  const [verified, setVerified] = useState<ReadonlyMap<string, Verified>>(new Map());
  const [busy, setBusy] = useState<string | null>(null);
  const nowS = Math.floor(useNow(1000) / 1000);
  if (snap.error !== null && snap.data === null)
    return <ErrorState error={snap.error} onRetry={snap.reload} />;
  if (snap.data === null) return <Skeleton lines={5} />;
  const data = snap.data;
  const dl = canDownload(
    info?.authority ?? 'F',
    info?.remote ?? false,
    data.download_remote_allowed,
  );

  const backupNow = async (): Promise<void> => {
    setBusy('now');
    try {
      const r = await rest.post<{ file: BackupFile | null }>('/admin/api/backups', mutation({}));
      toast.success(T.backups.done(r.file?.name ?? 'manual'));
      snap.reload();
    } catch (e) {
      toast.error(e, T.backups.now);
    } finally {
      setBusy(null);
    }
  };
  const verify = async (f: BackupFile): Promise<void> => {
    setBusy(f.name);
    try {
      const r = await rest.post<{ name: string; verified: Verified }>(
        `/admin/api/backups/${encodeURIComponent(f.name)}/verify`,
        mutation({}),
      );
      setVerified((m) => new Map(m).set(f.name, r.verified));
      if (r.verified.ok) toast.success(`${f.name}: ${T.backups.verified}.`);
      else toast.error(new Error(r.verified.detail), `${f.name}: ${T.backups.failedCheck}`);
    } catch (e) {
      toast.error(e, T.backups.verify);
    } finally {
      setBusy(null);
    }
  };

  const columns: Column<BackupFile>[] = [
    {
      id: 'name',
      header: 'File',
      cell: (f) => <code>{f.name}</code>,
      text: (f) => f.name,
      filterable: true,
    },
    {
      id: 'kind',
      header: 'Kind',
      cell: (f) => <Badge>{f.kind}</Badge>,
      text: (f) => f.kind,
      filterable: true,
      width: '100px',
    },
    {
      id: 'at',
      header: 'Taken',
      cell: (f) => <span title={formatDateTime(f.at)}>{formatAgo(f.at, nowS)}</span>,
      text: (f) => f.at,
      sortable: true,
      sortValue: (f) => f.at,
      width: '120px',
    },
    {
      id: 'size',
      header: 'Size',
      cell: (f) => formatBytes(f.size_bytes),
      text: (f) => f.size_bytes,
      sortable: true,
      sortValue: (f) => f.size_bytes,
      align: 'right',
      width: '90px',
    },
    {
      id: 'verified',
      header: 'Check',
      cell: (f) => {
        const v = verified.get(f.name) ?? f.verified;
        return v ? (
          <Badge tone={v.ok ? 'ok' : 'danger'} title={v.detail}>
            {v.ok ? T.backups.verified : T.backups.failedCheck}
          </Badge>
        ) : (
          <span className="adm-muted">{T.backups.notVerified}</span>
        );
      },
      text: (f) => ((verified.get(f.name) ?? f.verified)?.ok ? 'ok' : ''),
      width: '170px',
    },
    ...(canServer
      ? [
          {
            id: 'actions',
            header: '',
            cell: (f: BackupFile) => (
              <span className="adm-row">
                <Button
                  size="sm"
                  loading={busy === f.name}
                  aria-label={`${T.backups.verify} ${f.name}`}
                  onClick={() => void verify(f)}
                >
                  {T.backups.verify}
                </Button>
                {dl.ok ? (
                  <a
                    className="adm-btn adm-btn--secondary adm-btn--sm"
                    href={`/admin/api/backups/${encodeURIComponent(f.name)}/download`}
                    download={f.name}
                    aria-label={`${T.backups.download} ${f.name}`}
                  >
                    {T.backups.download}
                  </a>
                ) : (
                  <Button
                    size="sm"
                    disabled
                    title={dl.why}
                    aria-label={`${T.backups.download} ${f.name}`}
                  >
                    {T.backups.download}
                  </Button>
                )}
              </span>
            ),
            width: '210px',
          } satisfies Column<BackupFile>,
        ]
      : []),
  ];
  return (
    <section className="adm-stack" aria-label={T.backups.title}>
      <div className="adm-row">
        <h3 className="adm-h3">{T.backups.title}</h3>
        <span className="adm-spacer" />
        {data.free_bytes !== null && (
          <span className="adm-muted">{formatBytes(data.free_bytes)} free</span>
        )}
        {canServer && (
          <Button
            icon="download"
            variant="primary"
            loading={busy === 'now'}
            onClick={() => void backupNow()}
          >
            {T.backups.now}
          </Button>
        )}
      </div>
      <Table
        label="Backup files"
        columns={columns}
        rows={data.files.map((f) => ({ ...f, verified: verified.get(f.name) ?? f.verified }))}
        rowKey={(f) => f.name}
        emptyTitle={T.backups.empty}
        exportName="backups"
      />
      {!dl.ok && <p className="adm-muted">{dl.why}</p>}
      <p className="adm-muted">
        {T.backups.restoreHint} <code>{data.restore_command}</code>
      </p>
    </section>
  );
}

function Maintenance() {
  const snap = useSnapshot(
    (signal) => rest.get<MaintenanceResponse>('/admin/api/maintenance', { signal }),
    'maintenance',
    15_000,
  );
  if (snap.error !== null && snap.data === null)
    return <ErrorState error={snap.error} onRetry={snap.reload} />;
  if (snap.data === null) return <Skeleton lines={2} />;
  const m = snap.data;
  return (
    <section className="adm-stack" aria-label={T.backups.maintenance}>
      <h3 className="adm-h3">{T.backups.maintenance}</h3>
      <div className="adm-row">
        <Badge tone={m.enabled ? 'warn' : 'ok'}>
          {m.enabled ? T.backups.maintenanceOn : T.backups.maintenanceOff}
        </Badge>
        {m.allow_ips.length > 0 && (
          <span className="adm-muted">allowed addresses: {m.allow_ips.join(', ')}</span>
        )}
      </div>
      {!m.available && (
        <NeedsFeature wp={m.needs} title="Maintenance toggle">
          {T.backups.maintenanceNeeds}
        </NeedsFeature>
      )}
    </section>
  );
}

function Shutdown() {
  const canServer = useCan('act.server');
  const snap = useSnapshot(
    (signal) => rest.get<MaintenanceResponse>('/admin/api/maintenance', { signal }),
    'shutdown',
    15_000,
  );
  const [countdown, setCountdown] = useState('60');
  const [message, setMessage] = useState('');
  if (!canServer) return null;
  const restartAvailable = snap.data?.restart_available ?? false;
  const seconds = Number(countdown);
  const valid = Number.isInteger(seconds) && seconds >= 0 && seconds <= 3600;

  const start = (restart: boolean): void => {
    void runConfirmed({
      title: restart ? T.backups.restartTitle : T.backups.shutdownTitle,
      message: T.backups.shutdownHint,
      confirmLabel: restart ? T.backups.restartNow : T.backups.shutdownNow,
      danger: true,
      facts: { irreversible: true },
      run: () =>
        rest.post<{ countdown_s: number }>('/admin/api/shutdown', {
          countdown_s: seconds,
          ...(message.trim() ? { message: message.trim() } : {}),
          ...(restart ? { restart: true } : {}),
        }),
      success: (r) => T.backups.shutdownStarted(r.countdown_s),
    }).then(() => {
      snap.reload();
    });
  };
  const cancel = (): void => {
    void rest.post('/admin/api/shutdown', { cancel: true }).then(
      () => {
        toast.success(T.backups.cancelled);
        snap.reload();
      },
      (e: unknown) => {
        toast.error(e, T.backups.cancel);
      },
    );
  };
  return (
    <section className="adm-stack" aria-label={T.backups.shutdown}>
      <h3 className="adm-h3">{T.backups.shutdown}</h3>
      <div className="adm-row">
        <Field label={T.backups.countdown}>
          <NumberInput
            min={0}
            max={3600}
            value={countdown}
            onChange={(e) => {
              setCountdown(e.target.value);
            }}
          />
        </Field>
        <Field label={T.backups.message}>
          <TextInput
            value={message}
            maxLength={200}
            onChange={(e) => {
              setMessage(e.target.value);
            }}
          />
        </Field>
      </div>
      <div className="adm-row">
        <Button
          variant="danger"
          disabled={!valid}
          onClick={() => {
            start(false);
          }}
        >
          {T.backups.shutdownNow}
        </Button>
        <Button
          variant="danger"
          disabled={!valid || !restartAvailable}
          title={restartAvailable ? undefined : T.backups.noSupervisor}
          onClick={() => {
            start(true);
          }}
        >
          {T.backups.restartNow}
        </Button>
        <Button onClick={cancel}>{T.backups.cancel}</Button>
      </div>
      {!restartAvailable && <p className="adm-muted">{T.backups.noSupervisor}</p>}
      {snap.data?.restart_pending && <Badge tone="warn">{T.backups.restartPending}</Badge>}
    </section>
  );
}

export function BackupsAndMaintenance() {
  return (
    <div className="adm-stack">
      <Backups />
      <Maintenance />
      <Shutdown />
    </div>
  );
}
