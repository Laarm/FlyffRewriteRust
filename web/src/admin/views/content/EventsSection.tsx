// World & Content -> Rates & events -> Event flags (Plan/25 section 7.1): the named flags with their descriptions, the unnamed ones that have
// an override, and the generic events (500 and up, set by their timer, read only). A switch turns a flag on or off, a value flag takes a
// number; flag 1 is refused by the server and shown as protected. The routes are `GET /admin/api/events` and `PUT /admin/api/events/{id}`.
import { useState } from 'react';
import { runConfirmed } from '../../app/mutate';
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
import { useCan } from '../../stores/session';
import { toast } from '../../stores/toasts';
import { useSnapshot } from '../dashboard/useSnapshot';
import { listEvents, setEvent } from './api';
import {
  type EventRow,
  infoOf,
  isGeneric,
  isToggle,
  MAX_SETTABLE,
  PROTECTED_ID,
  settable,
  sortFlags,
} from './events';
import { T } from './strings';

const E = T.events;

function change(row: EventRow, state: number, done: () => void): void {
  const name = infoOf(row.id).name;
  void runConfirmed({
    title: E.confirmTitle(name, state),
    message: E.confirmBody,
    confirmLabel: E.set,
    run: () => setEvent(row.id, state),
    success: E.changed(name, state),
  }).then((o) => {
    if (o.ok) done();
  });
}

function ValueEditor({ row, done }: { readonly row: EventRow; readonly done: () => void }) {
  const [text, setText] = useState(String(row.state));
  const n = Number(text);
  const valid = text.trim() !== '' && Number.isInteger(n) && n >= 0 && n <= 255;
  return (
    <span className="content-rate">
      <NumberInput
        className="content-rate__input"
        aria-label={E.valueLabel(row.id)}
        min={0}
        max={255}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
        }}
        aria-invalid={!valid}
      />
      <Button
        size="sm"
        variant="primary"
        disabled={!valid || n === row.state}
        onClick={() => {
          change(row, n, done);
        }}
      >
        {E.set}
      </Button>
    </span>
  );
}

function SetOther({ done }: { readonly done: () => void }) {
  const [id, setId] = useState('');
  const [state, setState] = useState('1');
  const idN = Number(id);
  const stN = Number(state);
  const idOk =
    id.trim() !== '' &&
    Number.isInteger(idN) &&
    idN >= 0 &&
    idN <= MAX_SETTABLE &&
    idN !== PROTECTED_ID;
  const stOk = state.trim() !== '' && Number.isInteger(stN) && stN >= 0 && stN <= 255;
  return (
    <form
      className="content-form"
      onSubmit={(e) => {
        e.preventDefault();
        if (!idOk || !stOk) {
          toast.info(!idOk ? E.badId : E.badState);
          return;
        }
        change({ id: idN, key: null, state: stN, overridden: true }, stN, done);
      }}
    >
      <Field label={E.flagId} hint={E.otherHint}>
        <NumberInput
          min={0}
          max={MAX_SETTABLE}
          value={id}
          onChange={(e) => {
            setId(e.target.value);
          }}
        />
      </Field>
      <Field label={E.value}>
        <NumberInput
          min={0}
          max={255}
          value={state}
          onChange={(e) => {
            setState(e.target.value);
          }}
        />
      </Field>
      <Button type="submit" disabled={!idOk || !stOk}>
        {E.setOther}
      </Button>
    </form>
  );
}

export function EventsSection() {
  const canEdit = useCan('act.world');
  const snap = useSnapshot((signal) => listEvents(signal), 'events', 10_000);

  const columns: Column<EventRow>[] = [
    { id: 'id', header: E.id, cell: (r) => r.id, text: (r) => r.id, align: 'right', width: '56px' },
    {
      id: 'name',
      header: E.name,
      cell: (r) => (
        <span>
          <strong>{infoOf(r.id).name}</strong>
          {r.key !== null && (
            <>
              <br />
              <code className="adm-muted">{r.key}</code>
            </>
          )}
        </span>
      ),
      text: (r) => `${infoOf(r.id).name} ${r.key ?? ''}`,
      filterable: true,
      width: '200px',
    },
    {
      id: 'description',
      header: E.description,
      cell: (r) => <span className="adm-muted content-wrap">{infoOf(r.id).description}</span>,
      text: (r) => infoOf(r.id).description,
    },
    {
      id: 'state',
      header: E.state,
      cell: (r) => (
        <span className="content-state">
          {isToggle(r) ? (
            <Badge tone={r.state !== 0 ? 'ok' : 'neutral'}>{r.state !== 0 ? E.on : E.off}</Badge>
          ) : (
            <strong>{r.state}</strong>
          )}
          {r.overridden && <Badge tone="warn">{E.overridden}</Badge>}
        </span>
      ),
      text: (r) => r.state,
      width: '130px',
    },
  ];
  if (canEdit) {
    columns.push({
      id: 'change',
      header: '',
      cell: (r) => {
        if (r.id === PROTECTED_ID) return <Badge>{E.protectedFlag}</Badge>;
        if (isGeneric(r.id)) return <Badge>{E.generic}</Badge>;
        if (!settable(r.id)) return null;
        if (isToggle(r)) {
          const next = r.state !== 0 ? 0 : 1;
          return (
            <Button
              size="sm"
              aria-label={`${r.state !== 0 ? E.turnOff : E.turnOn}: ${infoOf(r.id).name}`}
              onClick={() => {
                change(r, next, snap.reload);
              }}
            >
              {r.state !== 0 ? E.turnOff : E.turnOn}
            </Button>
          );
        }
        return (
          <ValueEditor key={`${String(r.id)}:${String(r.state)}`} row={r} done={snap.reload} />
        );
      },
      width: '200px',
    });
  }

  return (
    <section className="content-section" aria-label={E.title}>
      <h3 className="content-h">{E.title}</h3>
      <p className="adm-muted">{E.hint}</p>
      {snap.error !== null && snap.data === null ? (
        <ErrorState error={snap.error} onRetry={snap.reload} />
      ) : snap.data === null ? (
        <Skeleton lines={6} />
      ) : (
        <>
          <Table
            label={E.title}
            columns={columns}
            rows={sortFlags(snap.data)}
            rowKey={(r) => String(r.id)}
            emptyTitle={E.empty}
          />
          {canEdit && <SetOther done={snap.reload} />}
        </>
      )}
    </section>
  );
}
