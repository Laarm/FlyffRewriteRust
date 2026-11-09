// Server -> Configuration (Plan/25 section 9.5; WP-ADMIN-26): 10's projection of the effective configuration (key, value, source,
// class). Live keys (`[rates]`, `[events]`, `events.flag.<id>`) are edited here (`SetRuntime` / `UnsetRuntime`, audited as
// `admin.config`); the other keys show what to do: "edit flyff.toml and restart" with the exact TOML to copy, or, for the keys
// `reload` applies, "edit the file, then Reload". "Compare with flyff.toml" shows the pending differences (`GET /admin/api/reload`,
// WP-OPS-16) without applying anything.
import { useMemo, useState } from 'react';
import { mutation, rest } from '../../api/rest';
import { copyText } from '../../app/clipboard';
import { runConfirmed } from '../../app/mutate';
import {
  Badge,
  Button,
  ChipGroup,
  Dialog,
  ErrorState,
  Field,
  Select,
  Skeleton,
  Table,
  TextInput,
  type Column,
} from '../../components';
import { useCan } from '../../stores/session';
import { toast } from '../../stores/toasts';
import { openDialog } from '../../stores/dialogs';
import type { ConfigClass, ConfigResponse, ConfigRow, ReloadReport } from './api';
import {
  displayValue,
  filterConfig,
  inputText,
  parseInput,
  sectionsOf,
  type ConfigFilter,
} from './configEdit';
import { T } from './strings';
import { useSnapshot } from '../dashboard/useSnapshot';

const EMPTY_ROWS: ReadonlyMap<string, ConfigRow> = new Map();

const CLASS_TONE: Record<ConfigClass, 'accent' | 'neutral' | 'warn'> = {
  live: 'accent',
  reload: 'neutral',
  restart: 'warn',
};

function EditDialog({
  row,
  close,
  onSaved,
}: {
  readonly row: ConfigRow;
  readonly close: () => void;
  readonly onSaved: (row: ConfigRow | null) => void;
}) {
  const [text, setText] = useState(inputText(row.value));
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const parsed = parseInput(row.kind, text);
  const save = async (): Promise<void> => {
    if (!parsed.ok) return;
    setBusy(true);
    setError(null);
    try {
      const r = await rest.put<{ row: ConfigRow | null }>(
        `/admin/api/config/${encodeURIComponent(row.key)}`,
        mutation({ value: parsed.value }, reason.trim() ? { text: reason.trim() } : undefined),
      );
      toast.success(T.config.saved(row.key));
      onSaved(r.row);
      close();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={`Edit ${row.key}`}
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
            disabled={!parsed.ok}
            onClick={() => void save()}
          >
            Save
          </Button>
        </>
      }
    >
      <div className="adm-stack">
        <p className="adm-muted">{row.doc}</p>
        <Field
          label="Value"
          hint={`${row.range ? `Accepted: ${row.range}. ` : ''}Default: ${displayValue(row.default)}.`}
          {...(!parsed.ok ? { error: parsed.error } : error ? { error } : {})}
        >
          {row.kind === 'bool' ? (
            <Select
              value={text}
              onChange={(e) => {
                setText(e.target.value);
              }}
            >
              <option value="true">true</option>
              <option value="false">false</option>
            </Select>
          ) : (
            <TextInput
              value={text}
              onChange={(e) => {
                setText(e.target.value);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void save();
              }}
            />
          )}
        </Field>
        <Field label="Reason (optional)">
          <TextInput
            value={reason}
            maxLength={200}
            onChange={(e) => {
              setReason(e.target.value);
            }}
          />
        </Field>
      </div>
    </Dialog>
  );
}

/** `true` when the report names no key, error or warning at all. */
function reportIsEmpty(r: ReloadReport): boolean {
  return (
    r.errors.length === 0 &&
    r.changed.length === 0 &&
    r.overridden.length === 0 &&
    r.restart_required.length === 0 &&
    r.warnings.length === 0
  );
}

/** The report of a reload (`applied`) or of the comparison (`!applied`: the same lists, nothing was done). */
function ReloadResult({
  r,
  applied = true,
}: {
  readonly r: ReloadReport;
  readonly applied?: boolean;
}) {
  return (
    <div className="adm-stack">
      {r.errors.length > 0 && (
        <>
          <strong>{T.config.reloadFailed}</strong>
          <ul>
            {r.errors.map((e) => (
              <li key={e}>{e}</li>
            ))}
          </ul>
        </>
      )}
      {r.changed.length > 0 && (
        <p>
          <strong>{applied ? T.config.applied : T.config.pending}</strong> {r.changed.join(', ')}
        </p>
      )}
      {r.overridden.length > 0 && (
        <p>
          <strong>{T.config.keptByOverride}</strong> {r.overridden.join(', ')}
        </p>
      )}
      {r.restart_required.length > 0 && (
        <p>
          <strong>{T.config.needsRestart}</strong> {r.restart_required.join(', ')}
        </p>
      )}
      {r.warnings.length > 0 && (
        <>
          <strong>{T.config.warnings}</strong>
          <ul>
            {r.warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        </>
      )}
      {reportIsEmpty(r) && <p>{T.config.matches}</p>}
    </div>
  );
}

export function Config() {
  const canEdit = useCan('act.server');
  const snap = useSnapshot(
    (signal) => rest.get<ConfigResponse>('/admin/api/config', { signal }),
    'config',
    30_000,
  );
  // what a save or a reset returned replaces its row until the next answer of the server (which is newer)
  const [edited, setEdited] = useState<{
    readonly base: ConfigResponse | null;
    readonly rows: ReadonlyMap<string, ConfigRow>;
  }>({ base: null, rows: EMPTY_ROWS });
  const [filter, setFilter] = useState<ConfigFilter>({ text: '', section: '', cls: 'all' });
  const base = snap.data;
  const patches = edited.base === base ? edited.rows : EMPTY_ROWS;
  const rows = useMemo(
    () => (base ? base.keys.map((r) => patches.get(r.key) ?? r) : null),
    [base, patches],
  );
  const setRow = (row: ConfigRow | null): void => {
    if (row === null || base === null) return;
    setEdited({ base, rows: new Map(patches).set(row.key, row) });
  };
  const shown = useMemo(() => (rows ? filterConfig(rows, filter) : []), [rows, filter]);
  if (snap.error !== null && rows === null)
    return <ErrorState error={snap.error} onRetry={snap.reload} />;
  if (rows === null || snap.data === null) return <Skeleton lines={8} />;

  const reset = (row: ConfigRow): void => {
    void runConfirmed({
      title: `Back to the file value: ${row.key}`,
      message:
        'The runtime override is removed; the value from flyff.toml (or the default) applies again.',
      reason: 'optional',
      run: async (given) => {
        const r = await rest.delete<{ row: ConfigRow | null }>(
          `/admin/api/config/${encodeURIComponent(row.key)}`,
          mutation({}, given.reason),
        );
        return r.row;
      },
      success: T.config.resetDone(row.key),
    }).then((o) => {
      if (o.ok) setRow(o.value);
    });
  };

  const columns: Column<ConfigRow>[] = [
    {
      id: 'key',
      header: 'Key',
      cell: (r) => (
        <>
          <code>{r.key}</code>
          <div className="adm-muted adm-small">{r.doc}</div>
        </>
      ),
      text: (r) => r.key,
      sortable: true,
    },
    {
      id: 'value',
      header: 'Value',
      cell: (r) => (
        <span className="adm-mono" title={displayValue(r.value)}>
          {displayValue(r.value)}
        </span>
      ),
      text: (r) => displayValue(r.value),
      width: '220px',
    },
    {
      id: 'source',
      header: 'Source',
      cell: (r) => (
        <Badge
          tone={r.source === 'override' ? 'accent' : 'neutral'}
          {...(r.source_detail ? { title: r.source_detail } : {})}
        >
          {r.source}
        </Badge>
      ),
      text: (r) => r.source,
      width: '90px',
    },
    {
      id: 'class',
      header: 'Class',
      cell: (r) => <Badge tone={CLASS_TONE[r.class]}>{r.class}</Badge>,
      text: (r) => r.class,
      width: '80px',
    },
    {
      id: 'actions',
      header: '',
      cell: (r) => (
        <span className="adm-row">
          {r.editable && canEdit && (
            <Button
              size="sm"
              aria-label={`Edit ${r.key}`}
              onClick={(e) => {
                e.stopPropagation();
                openDialog((close) => <EditDialog row={r} close={close} onSaved={setRow} />);
              }}
            >
              {T.config.edit}
            </Button>
          )}
          {r.editable && canEdit && r.source === 'override' && (
            <Button
              size="sm"
              variant="ghost"
              aria-label={`${T.config.reset}: ${r.key}`}
              onClick={(e) => {
                e.stopPropagation();
                reset(r);
              }}
            >
              {T.config.reset}
            </Button>
          )}
          {!r.editable && r.snippet && (
            <Button
              size="sm"
              variant="ghost"
              aria-label={`${T.config.copy}: ${r.key}`}
              title={r.class === 'restart' ? T.config.restartHint : T.config.reloadHint}
              onClick={(e) => {
                e.stopPropagation();
                if (copyText(r.snippet)) toast.success(T.config.copied);
                else toast.error(new Error(T.config.copyFailed));
              }}
            >
              {T.config.copy}
            </Button>
          )}
        </span>
      ),
      width: '230px',
    },
  ];

  const counts = snap.data.counts;
  return (
    <div className="adm-stack">
      <div className="adm-row">
        <TextInput
          aria-label={T.config.filter}
          placeholder={T.config.filter}
          value={filter.text}
          onChange={(e) => {
            setFilter({ ...filter, text: e.target.value });
          }}
        />
        <Select
          aria-label={T.config.section}
          value={filter.section}
          onChange={(e) => {
            setFilter({ ...filter, section: e.target.value });
          }}
        >
          <option value="">{`${T.config.section}: ${T.config.all}`}</option>
          {sectionsOf(rows).map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </Select>
        <ChipGroup
          label="Class"
          chips={[
            { value: 'live', label: `live (${String(counts.live)})` },
            { value: 'reload', label: `reload (${String(counts.reload)})` },
            { value: 'restart', label: `restart (${String(counts.restart)})` },
            { value: 'overridden', label: `overridden (${String(snap.data.overrides)})` },
          ]}
          value={filter.cls === 'all' ? null : filter.cls}
          onChange={(v) => {
            setFilter({ ...filter, cls: v ?? 'all' });
          }}
        />
        <span className="adm-spacer" />
        {canEdit && (
          <Button
            icon="search"
            onClick={() => {
              void rest.get<ReloadReport>('/admin/api/reload').then(
                (r) => {
                  openDialog((close) => (
                    <Dialog
                      title={reportIsEmpty(r) ? T.config.noDifference : T.config.compareTitle}
                      onClose={close}
                      dismissOnOverlay
                      footer={<Button onClick={close}>Close</Button>}
                    >
                      <ReloadResult r={r} applied={false} />
                      {!reportIsEmpty(r) && <p className="adm-muted">{T.config.compareHint}</p>}
                    </Dialog>
                  ));
                },
                (e: unknown) => {
                  toast.error(e, T.config.compare);
                },
              );
            }}
          >
            {T.config.compare}
          </Button>
        )}
        {canEdit && (
          <Button
            icon="refresh"
            onClick={() => {
              void rest.post<ReloadReport>('/admin/api/reload', {}).then(
                (r) => {
                  toast.success(T.config.reloaded(r.changed.length, r.restart_required.length));
                  openDialog((close) => (
                    <Dialog
                      title={T.config.reload}
                      onClose={close}
                      dismissOnOverlay
                      footer={<Button onClick={close}>Close</Button>}
                    >
                      <ReloadResult r={r} />
                    </Dialog>
                  ));
                  snap.reload();
                },
                (e: unknown) => {
                  toast.error(e, T.config.reload);
                },
              );
            }}
          >
            {T.config.reload}
          </Button>
        )}
      </div>
      {!snap.data.persist_overrides && <p className="adm-muted">{T.config.notPersisted}</p>}
      <Table
        label="Configuration"
        columns={columns}
        rows={shown}
        rowKey={(r) => r.key}
        emptyTitle="No key matches"
        exportName="configuration"
      />
    </div>
  );
}
