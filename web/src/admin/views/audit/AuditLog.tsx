// Audit -> Audit log (Plan/25 section 10.2; WP-ADMIN-29): the search of `logs/audit/*.jsonl` through the per-day index, with a time
// range, the event groups, actor, character, account, serial and free text; a table with time, event, actor, subject and summary;
// the raw JSON of a record on selection; JSONL and CSV exports of the same filters; and Undo where the action has an inverse.
import { useMemo, useState, type SyntheticEvent } from 'react';
import type { TabProps } from '../../app/views';
import {
  Badge,
  Button,
  Checkbox,
  Field,
  Select,
  Table,
  TextInput,
  usePagedList,
  type Column,
} from '../../components';
import { useCan } from '../../stores/session';
import { GROUPS, type AuditRow, type Group } from './api';
import { entryOfRow } from './collapse';
import { RecordDetail, stateTone, SubjectLinks, formatStamp } from './Detail';
import {
  buildQuery,
  EMPTY_FILTERS,
  exportUrl,
  filtersFromParams,
  RANGES,
  type FilterErrors,
  type Filters,
  type RangeId,
} from './filters';
import { GROUP_LABELS, T } from './strings';
import { UndoButton, useUndone } from './Undo';

type Query = Readonly<Record<string, string | number>>;

export function AuditLog({ route }: TabProps) {
  const canAll = useCan('view.audit');
  const undone = useUndone();
  const [draft, setDraft] = useState<Filters>(() => filtersFromParams(route.params));
  const [errors, setErrors] = useState<FilterErrors>({});
  // the query is fixed when the operator presses Search ("last 24 hours" is relative to that moment)
  const [applied, setApplied] = useState<Query | null>(() => {
    const built = buildQuery(filtersFromParams(route.params), Date.now());
    return built.ok ? built.query : null;
  });
  const [selected, setSelected] = useState<string | null>(null);
  const list = usePagedList<AuditRow>({
    path: '/admin/api/audit',
    ...(applied === null ? {} : { query: applied }),
    enabled: applied !== null,
    limit: 100,
  });

  const apply = (f: Filters): void => {
    const built = buildQuery(f, Date.now());
    if (!built.ok) {
      setErrors(built.errors);
      return;
    }
    setErrors({});
    setApplied(built.query);
    setSelected(null);
  };
  const set = <K extends keyof Filters>(key: K, value: Filters[K]): void => {
    setDraft((d) => ({ ...d, [key]: value }));
  };
  const toggle = (g: Group): void => {
    set(
      'groups',
      draft.groups.includes(g) ? draft.groups.filter((x) => x !== g) : [...draft.groups, g],
    );
  };

  const columns: Column<AuditRow>[] = [
    {
      id: 'time',
      header: 'Time',
      cell: (r) => <span className="adm-nowrap">{formatStamp(r.ts)}</span>,
      text: (r) => formatStamp(r.ts),
      width: '190px',
    },
    {
      id: 'ev',
      header: 'Event',
      cell: (r) => <span className="adm-audit__ev">{r.ev}</span>,
      text: (r) => r.ev,
      width: '170px',
    },
    {
      id: 'state',
      header: 'State',
      cell: (r) => (r.state === null ? null : <Badge tone={stateTone(r.state)}>{r.state}</Badge>),
      text: (r) => r.state ?? '',
      width: '100px',
    },
    {
      id: 'actor',
      header: 'Actor',
      cell: (r) => r.actor ?? '',
      text: (r) => r.actor ?? '',
      width: '130px',
    },
    { id: 'subject', header: 'Subject', cell: (r) => <SubjectLinks row={r} />, width: '150px' },
    {
      id: 'summary',
      header: 'Summary',
      cell: (r) => (
        <span className="adm-ellipsis" title={r.summary}>
          {r.summary}
        </span>
      ),
      text: (r) => r.summary,
    },
    {
      id: 'undo',
      header: '',
      cell: (r) => <UndoButton entry={entryOfRow(r)} undone={undone} onUndone={list.reload} />,
      width: '90px',
    },
  ];

  const row = useMemo(
    () => list.rows.find((r) => r.id === selected) ?? null,
    [list.rows, selected],
  );

  return (
    <div className="adm-stack">
      {!canAll && <p className="adm-muted">{T.log.own}</p>}
      <form
        className="adm-audit__filters"
        aria-label={T.log.filters}
        onSubmit={(e: SyntheticEvent) => {
          e.preventDefault();
          apply(draft);
        }}
      >
        <Field label={T.log.range}>
          <Select
            value={draft.range}
            onChange={(e) => {
              set('range', e.target.value as RangeId);
            }}
          >
            {RANGES.map((r) => (
              <option key={r.id} value={r.id}>
                {r.label}
              </option>
            ))}
          </Select>
        </Field>
        {draft.range === 'custom' && (
          <>
            <Field label={T.log.from} {...(errors.from ? { error: errors.from } : {})}>
              <TextInput
                type="datetime-local"
                value={draft.from}
                onChange={(e) => {
                  set('from', e.target.value);
                }}
              />
            </Field>
            <Field label={T.log.to} {...(errors.to ? { error: errors.to } : {})}>
              <TextInput
                type="datetime-local"
                value={draft.to}
                onChange={(e) => {
                  set('to', e.target.value);
                }}
              />
            </Field>
          </>
        )}
        <Field label={T.log.actor} hint={T.log.actorHint}>
          <TextInput
            value={draft.actor}
            onChange={(e) => {
              set('actor', e.target.value);
            }}
          />
        </Field>
        <Field label={T.log.character} {...(errors.character ? { error: errors.character } : {})}>
          <TextInput
            inputMode="numeric"
            value={draft.character}
            onChange={(e) => {
              set('character', e.target.value);
            }}
          />
        </Field>
        <Field label={T.log.account} {...(errors.account ? { error: errors.account } : {})}>
          <TextInput
            inputMode="numeric"
            value={draft.account}
            onChange={(e) => {
              set('account', e.target.value);
            }}
          />
        </Field>
        <Field label={T.log.serial} {...(errors.serial ? { error: errors.serial } : {})}>
          <TextInput
            inputMode="numeric"
            value={draft.serial}
            onChange={(e) => {
              set('serial', e.target.value);
            }}
          />
        </Field>
        <div className="adm-audit__wide">
          <Field label={T.log.text} hint={T.log.textHint}>
            <TextInput
              value={draft.text}
              onChange={(e) => {
                set('text', e.target.value);
              }}
            />
          </Field>
        </div>
        <fieldset className="adm-audit__groups adm-audit__wide">
          <legend>{T.log.groups}</legend>
          {GROUPS.map((g) => (
            <Checkbox
              key={g}
              label={GROUP_LABELS[g]}
              checked={draft.groups.includes(g)}
              onChange={() => {
                toggle(g);
              }}
            />
          ))}
        </fieldset>
        <div className="adm-row adm-audit__wide">
          <Button type="submit" variant="primary">
            {T.log.search}
          </Button>
          <Button
            type="button"
            variant="ghost"
            onClick={() => {
              setDraft(EMPTY_FILTERS);
              apply(EMPTY_FILTERS);
            }}
          >
            {T.log.reset}
          </Button>
          <span className="adm-grow" />
          {applied !== null && (
            <>
              <a
                className="adm-btn adm-btn--secondary adm-btn--sm"
                href={exportUrl(applied, 'jsonl')}
                download
              >
                {T.log.exportJsonl}
              </a>
              <a
                className="adm-btn adm-btn--secondary adm-btn--sm"
                href={exportUrl(applied, 'csv')}
                download
              >
                {T.log.exportCsv}
              </a>
            </>
          )}
        </div>
      </form>
      <Table
        label={T.log.table}
        columns={columns}
        rows={list.rows}
        rowKey={(r) => r.id}
        loading={list.loading}
        error={list.error}
        onRetry={list.reload}
        emptyTitle={T.log.empty}
        emptyHint={T.log.emptyHint}
        hasMore={list.hasMore}
        loadingMore={list.loadingMore}
        onLoadMore={list.loadMore}
        onRowActivate={(r) => {
          setSelected(r.id);
        }}
        selectedKey={selected}
      />
      {row === null ? (
        list.rows.length > 0 && <p className="adm-muted">{T.log.noRows}</p>
      ) : (
        <RecordDetail
          row={row}
          onClose={() => {
            setSelected(null);
          }}
        />
      )}
    </div>
  );
}
