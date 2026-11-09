// Server -> Scheduler (Plan/25 section 9.6; WP-ADMIN-26): the scheduler's jobs (`Query::ListJobs`): name, kind, schedule, last run,
// run count and next run, with "Run now" for the jobs flagged runnable (`AdminCmd::RunJob`, `act.server`; audited as `admin.job`).
// A job has no result of its own: a job that fails panics the game thread, so the page shows runs and times only.
import { useState } from 'react';
import { mutation, rest } from '../../api/rest';
import { runConfirmed } from '../../app/mutate';
import { useNow } from '../../app/polling';
import { Badge, Button, ErrorState, Skeleton, Table, type Column } from '../../components';
import { useCan } from '../../stores/session';
import { formatAgo, formatDateTime, formatDuration } from '../dashboard/format';
import { useSnapshot } from '../dashboard/useSnapshot';
import type { Job, JobsResponse } from './api';
import { T } from './strings';

/** `in 4m 12s` for a future time, `due` for a past one, `-` for none (milliseconds). */
export function nextText(nextMs: number | null, nowMs: number): string {
  if (nextMs === null) return '-';
  const d = Math.round((nextMs - nowMs) / 1000);
  return d <= 0 ? 'due' : `in ${formatDuration(d)}`;
}

export function kindTone(kind: Job['kind']): 'accent' | 'neutral' | 'ok' {
  return kind === 'wall' ? 'accent' : kind === 'slot' ? 'neutral' : 'ok';
}

export function Scheduler() {
  const canRun = useCan('act.server');
  const snap = useSnapshot(
    (signal) => rest.get<JobsResponse>('/admin/api/jobs', { signal }),
    'jobs',
    5000,
  );
  const [local, setLocal] = useState<ReadonlyMap<string, Job>>(new Map());
  const nowMs = useNow(1000);
  if (snap.error !== null && snap.data === null)
    return <ErrorState error={snap.error} onRetry={snap.reload} />;
  if (snap.data === null) return <Skeleton lines={8} />;
  const nowS = Math.floor(nowMs / 1000);
  // a fresh answer of the server replaces what a run returned meanwhile
  const rows = snap.data.jobs.map((j) => local.get(j.name) ?? j);

  const run = (job: Job): void => {
    void runConfirmed({
      title: T.jobs.runTitle(job.name),
      message: T.jobs.runMessage,
      confirmLabel: T.jobs.run,
      reason: 'optional',
      run: async (given) => {
        const r = await rest.post<{ job: Job | null }>(
          `/admin/api/jobs/${encodeURIComponent(job.name)}/run`,
          mutation({}, given.reason),
        );
        return r.job;
      },
      success: T.jobs.ran(job.name),
    }).then((o) => {
      if (o.ok && o.value) {
        const updated = o.value;
        setLocal((m) => new Map(m).set(updated.name, updated));
      }
      snap.reload();
    });
  };

  const columns: Column<Job>[] = [
    {
      id: 'name',
      header: 'Job',
      cell: (j) => <code>{j.name}</code>,
      text: (j) => j.name,
      sortable: true,
      filterable: true,
    },
    {
      id: 'kind',
      header: 'Kind',
      cell: (j) => <Badge tone={kindTone(j.kind)}>{j.kind}</Badge>,
      text: (j) => j.kind,
      filterable: true,
      width: '90px',
    },
    { id: 'row', header: 'Row', cell: (j) => j.row, text: (j) => j.row, width: '80px' },
    { id: 'schedule', header: 'Schedule', cell: (j) => j.schedule, text: (j) => j.schedule },
    {
      id: 'last',
      header: 'Last run',
      cell: (j) =>
        j.last_run_unix_ms === null ? (
          <span className="adm-muted">{T.jobs.never}</span>
        ) : (
          <span title={formatDateTime(Math.floor(j.last_run_unix_ms / 1000))}>
            {formatAgo(Math.floor(j.last_run_unix_ms / 1000), nowS)}
          </span>
        ),
      text: (j) => j.last_run_unix_ms ?? 0,
      sortable: true,
      sortValue: (j) => j.last_run_unix_ms ?? 0,
      width: '120px',
    },
    {
      id: 'runs',
      header: 'Runs',
      cell: (j) => j.runs,
      text: (j) => j.runs,
      sortable: true,
      sortValue: (j) => j.runs,
      align: 'right',
      width: '80px',
    },
    {
      id: 'next',
      header: 'Next run',
      cell: (j) => nextText(j.next_run_unix_ms, nowMs),
      text: (j) => j.next_run_unix_ms ?? 0,
      sortable: true,
      sortValue: (j) => j.next_run_unix_ms ?? Number.MAX_SAFE_INTEGER,
      width: '110px',
    },
    {
      id: 'gate',
      header: '',
      cell: (j) => (j.gate_open ? null : <Badge tone="warn">{T.jobs.closed}</Badge>),
      text: (j) => (j.gate_open ? '' : T.jobs.closed),
      width: '100px',
    },
    ...(canRun
      ? [
          {
            id: 'run',
            header: '',
            cell: (j: Job) =>
              j.runnable ? (
                <Button
                  size="sm"
                  aria-label={`${T.jobs.run}: ${j.name}`}
                  onClick={() => {
                    run(j);
                  }}
                >
                  {T.jobs.run}
                </Button>
              ) : (
                <span className="adm-muted" title={T.jobs.notRunnable}>
                  {'-'}
                </span>
              ),
            width: '100px',
          } satisfies Column<Job>,
        ]
      : []),
  ];
  return (
    <div className="adm-stack">
      <Table
        label="Scheduler jobs"
        columns={columns}
        rows={rows}
        rowKey={(j) => j.name}
        exportName="scheduler-jobs"
      />
      <p className="adm-muted">
        Manual runs are counted apart (column Runs includes them) and recorded in the audit log as
        admin.job.
      </p>
    </div>
  );
}
