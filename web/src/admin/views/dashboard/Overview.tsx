// The dashboard's Overview (Plan/25 section 9.1, 9.2; WP-ADMIN-25): cards for the server, the players, the tick, the database,
// memory, the asset cache and the disk, six small charts over the last 1 to 24 hours (the history switches to the minute
// resolution above two hours), the active alerts and the quick actions (broadcast a notice, save everyone, back up now).
// The numbers of the cards are the live status of the top bar (`StatusSubscribe`, else the polled `/admin/api/status`), so the
// dashboard and the pills never disagree.
import { useState, type ReactNode } from 'react';
import { mutation, rest } from '../../api/rest';
import { navigate } from '../../stores/route';
import { openDialog } from '../../stores/dialogs';
import { useCan } from '../../stores/session';
import {
  LIVE_STALE_MS,
  formatMs,
  useStatus,
  type LiveStatus,
  type ServerStatus,
} from '../../stores/status';
import { toast } from '../../stores/toasts';
import {
  Badge,
  Button,
  ChipGroup,
  Dialog,
  ErrorState,
  Field,
  Skeleton,
  Sparkline,
  TextArea,
} from '../../components';
import { getHistory, SPANS, spanSeconds, type History, type SpanId } from './api';
import { LineChart } from './LineChart';
import {
  formatAgo,
  formatBytes,
  formatDuration,
  formatRate,
  formatMicros,
  percent,
} from './format';
import { D } from './strings';
import { useSnapshot } from './useSnapshot';
import { useNow } from '../../app/polling';

function Card({ title, children }: { readonly title: string; readonly children: ReactNode }) {
  return (
    <section className="adm-card" aria-label={title}>
      <h3>{title}</h3>
      <div className="adm-card__value">{children}</div>
    </section>
  );
}

/** What the cards show of the live status: the socket's tick when it is fresh, else the polled answer. */
export interface Vitals {
  readonly in_world: number;
  readonly lobby: number;
  readonly tick_p50_us: number;
  readonly tick_p99_us: number;
  readonly db_degraded: boolean;
  readonly db_queue: number;
  readonly shutdown_in_s: number | null;
}

export function vitalsOf(
  status: ServerStatus | null,
  live: LiveStatus | null,
  nowMs: number,
): Vitals | null {
  if (live && nowMs - live.at < LIVE_STALE_MS) return live;
  return status;
}

function Meter({
  value,
  total,
  label,
}: {
  readonly value: number;
  readonly total: number;
  readonly label: string;
}) {
  const p = percent(value, total);
  return (
    <div
      className="adm-meter"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={p}
    >
      <div
        className={`adm-meter__fill ${p >= 90 ? 'is-high' : ''}`}
        style={{ width: `${String(p)}%` }}
      />
    </div>
  );
}

function BroadcastDialog({ close }: { readonly close: () => void }) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const send = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await rest.post('/admin/api/notice', { text: text.trim() });
      toast.success(D.broadcastDone);
      close();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={D.broadcastTitle}
      onClose={close}
      busy={busy}
      footer={
        <>
          <Button onClick={close} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={text.trim() === ''}
            onClick={() => void send()}
          >
            {D.broadcastSend}
          </Button>
        </>
      }
    >
      <Field label={D.broadcastText} hint={D.broadcastHint} {...(error ? { error } : {})}>
        <TextArea
          rows={3}
          maxLength={512}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
        />
      </Field>
    </Dialog>
  );
}

function QuickActions() {
  const canBroadcast = useCan('act.broadcast');
  const canServer = useCan('act.server');
  const [busy, setBusy] = useState<string | null>(null);
  if (!canBroadcast && !canServer) return null;
  const run = async (id: string, task: () => Promise<string>): Promise<void> => {
    setBusy(id);
    try {
      toast.success(await task());
    } catch (e) {
      toast.error(e, 'Action failed');
    } finally {
      setBusy(null);
    }
  };
  return (
    <div className="adm-row adm-quick" role="group" aria-label={D.quick}>
      {canBroadcast && (
        <Button
          icon="message"
          onClick={() => {
            openDialog((close) => <BroadcastDialog close={close} />);
          }}
        >
          {D.broadcast}
        </Button>
      )}
      {canServer && (
        <>
          <Button
            icon="check"
            loading={busy === 'save'}
            onClick={() =>
              void run('save', async () => {
                await rest.post('/admin/api/console', { line: 'save-all' });
                return D.saveDone;
              })
            }
          >
            {D.saveAll}
          </Button>
          <Button
            icon="download"
            loading={busy === 'backup'}
            onClick={() =>
              void run('backup', async () => {
                const r = await rest.post<{ file: { name: string } | null }>(
                  '/admin/api/backups',
                  mutation({}),
                );
                return D.backupDone(r.file?.name ?? 'manual');
              })
            }
          >
            {D.backupNow}
          </Button>
        </>
      )}
    </div>
  );
}

function Cards({
  v,
  h,
  nowS,
}: {
  readonly v: Vitals | null;
  readonly h: History;
  readonly nowS: number;
}) {
  const status = useStatus((s) => s.status);
  const c = h.current;
  const col = h.columns;
  const mb = (n: number): string => formatBytes(n);
  return (
    <div className="adm-cards">
      <Card title="Server">
        {status ? (
          <>
            <Badge tone={status.running ? 'ok' : 'warn'}>{status.state}</Badge>
            <div className="adm-muted">
              up {formatDuration(status.uptime_s)}, build <code>{status.build}</code>
            </div>
            <div className="adm-muted">
              Maintenance: {c.maintenance ? 'on' : 'off'}
              {v?.shutdown_in_s != null && <> | shutdown in {formatDuration(v.shutdown_in_s)}</>}
            </div>
          </>
        ) : (
          <span className="adm-muted">No status yet</span>
        )}
      </Card>
      <Card title="Players">
        <span data-testid="players-online">{v ? v.in_world : '-'}</span> in the world
        <div className="adm-muted">
          {v ? v.lobby : '-'} in the lobby | peak {c.peak_in_world}
        </div>
        <div className="adm-muted">{c.logins_last_hour} logins in the last hour</div>
      </Card>
      <Card title="Tick">
        <span data-testid="tick-p50">{v ? formatMs(v.tick_p50_us) : '-'}</span> p50
        <div className="adm-muted" data-testid="tick-p99">
          {v ? formatMs(v.tick_p99_us) : '-'} p99
        </div>
        <Sparkline values={col.tick_p99_us} label="Tick p99 over the selected window" width={160} />
      </Card>
      <Card title="Database">
        {v?.db_degraded ? <Badge tone="danger">degraded</Badge> : <Badge tone="ok">ok</Badge>}
        <div className="adm-muted">write queue {v ? v.db_queue : '-'}</div>
        <div className="adm-muted">
          WAL {mb(c.db_wal_bytes)} | {c.db_checkpoints} checkpoints
        </div>
        <div className="adm-muted">
          last backup{' '}
          {c.last_backup_unix === null ? 'none found' : formatAgo(c.last_backup_unix, nowS)}
        </div>
      </Card>
      <Card title="Memory">
        {mb(c.rss_bytes)}
        <div className="adm-muted">resident</div>
        <Sparkline
          values={col.rss_bytes}
          label="Resident memory over the selected window"
          width={160}
        />
      </Card>
      <Card title="Cache">
        {mb(c.cache_bytes)}
        <div className="adm-muted">of {mb(c.cache_budget_bytes)} budget</div>
        <Meter
          value={c.cache_bytes}
          total={c.cache_budget_bytes}
          label="Asset cache against its budget"
        />
      </Card>
      <Card title="Disk">
        {c.disk_free_bytes === null ? '-' : mb(c.disk_free_bytes)}
        <div className="adm-muted">free in the data directory</div>
      </Card>
    </div>
  );
}

function Charts({ h }: { readonly h: History }) {
  const col = h.columns;
  const ms = (v: number): string => formatMicros(v);
  const count = (v: number): string => String(Math.round(v));
  return (
    <div className="adm-charts">
      <LineChart
        title="Players"
        t={col.t}
        format={count}
        minMax={5}
        series={[
          { id: 'world', label: 'in world', values: col.in_world },
          { id: 'lobby', label: 'lobby', values: col.lobby },
        ]}
      />
      <LineChart
        title="Tick time"
        t={col.t}
        format={ms}
        minMax={5000}
        series={[
          { id: 'p50', label: 'p50', values: col.tick_p50_us },
          { id: 'p99', label: 'p99', values: col.tick_p99_us },
        ]}
      />
      <LineChart
        title="Database write queue"
        t={col.t}
        format={count}
        minMax={10}
        series={[{ id: 'q', label: 'queued', values: col.db_queue }]}
      />
      <LineChart
        title="Memory"
        t={col.t}
        format={formatBytes}
        minMax={64 << 20}
        series={[{ id: 'rss', label: 'resident', values: col.rss_bytes }]}
      />
      <LineChart
        title="Network"
        t={col.t}
        format={formatRate}
        minMax={1024}
        series={[
          { id: 'in', label: 'in', values: col.bytes_in_s },
          { id: 'out', label: 'out', values: col.bytes_out_s },
        ]}
      />
      <LineChart
        title="Sessions"
        t={col.t}
        format={count}
        minMax={5}
        series={[
          { id: 'gw', label: 'player sockets', values: col.ws_sessions },
          { id: 'admin', label: 'console tabs', values: col.admin_ws },
        ]}
      />
    </div>
  );
}

export function Overview() {
  const [span, setSpan] = useState<SpanId>('1h');
  const snap = useSnapshot((signal) => getHistory(spanSeconds(span), signal), span, 10_000);
  const status = useStatus((s) => s.status);
  const live = useStatus((s) => s.live);
  const nowMs = useNow(1000);
  const alertCount = useStatus((s) => s.alerts);
  const liveAlerts = live?.alerts ?? alertCount ?? 0;
  const h = snap.data;
  const nowS = Math.floor(nowMs / 1000);
  const v = vitalsOf(status, live, nowMs);

  return (
    <div className="adm-stack adm-dashboard">
      <div className="adm-row adm-dashboard__bar">
        <ChipGroup
          label="Time window"
          chips={SPANS.map((s) => ({ value: s.id, label: s.label }))}
          value={span}
          onChange={(id) => {
            if (id !== null) setSpan(id);
          }}
        />
        <Button icon="refresh" size="sm" variant="ghost" onClick={snap.reload}>
          Refresh
        </Button>
        {liveAlerts > 0 && (
          <button
            type="button"
            className="adm-banner-link"
            onClick={() => {
              navigate({ category: 'dashboard', tab: 'alerts', params: {} });
            }}
          >
            <Badge tone="warn">{D.alertsBanner(liveAlerts)}</Badge> see the alerts
          </button>
        )}
        <span className="adm-spacer" />
        <QuickActions />
      </div>
      {snap.error !== null && h === null ? (
        <ErrorState error={snap.error} onRetry={snap.reload} />
      ) : h === null ? (
        <Skeleton lines={6} />
      ) : (
        <>
          <Cards v={v} h={h} nowS={nowS} />
          <Charts h={h} />
          <p className="adm-muted adm-dashboard__note">
            {D.historyNote(formatDuration(h.since_start_s))}
          </p>
        </>
      )}
    </div>
  );
}
