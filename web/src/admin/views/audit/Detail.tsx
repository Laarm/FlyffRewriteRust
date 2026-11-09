// The panel under a table that shows one record in full: who, when, why, the records it is about and the raw JSON (Plan/25 section
// 10.2 "the raw JSON on expand"). A "My actions" entry shows its request record as well as its outcome.
import { copyText } from '../../app/clipboard';
import { getRoute, navigate } from '../../stores/route';
import { withRecord, type RecordKind } from '../../app/route';
import { Badge, Button, type Tone } from '../../components';
import { toast } from '../../stores/toasts';
import { formatDateTime } from '../dashboard/format';
import type { AuditRow } from './api';
import { T } from './strings';

export function stateTone(state: string | null): Tone {
  if (state === 'done') return 'ok';
  if (state === 'failed') return 'danger';
  if (state === 'requested') return 'warn';
  return 'neutral';
}

/** `2026-10-23 09:20:47.123` of a record's Unix milliseconds. */
export function formatStamp(tsMs: number): string {
  const ms = String(Math.abs(Math.trunc(tsMs)) % 1000).padStart(3, '0');
  return `${formatDateTime(Math.floor(tsMs / 1000))}.${ms}`;
}

function open(kind: RecordKind, id: number): void {
  const route = getRoute();
  if (route) navigate(withRecord(route, { kind, id: String(id) }));
}

/** The characters and accounts of a record as buttons that open their drawers. */
export function SubjectLinks({ row }: { readonly row: AuditRow }) {
  const { characters, accounts } = row.subject;
  if (characters.length === 0 && accounts.length === 0) return <span className="adm-muted">-</span>;
  return (
    <span className="adm-audit__subjects">
      {characters.slice(0, 4).map((c) => (
        <button
          key={`c${String(c)}`}
          type="button"
          className="adm-linkbtn"
          aria-label={`Open character ${String(c)}`}
          onClick={(e) => {
            e.stopPropagation();
            open('character', c);
          }}
        >
          {`char ${String(c)}`}
        </button>
      ))}
      {accounts.slice(0, 3).map((a) => (
        <button
          key={`a${String(a)}`}
          type="button"
          className="adm-linkbtn"
          aria-label={`Open account ${String(a)}`}
          onClick={(e) => {
            e.stopPropagation();
            open('account', a);
          }}
        >
          {`acct ${String(a)}`}
        </button>
      ))}
      {characters.length > 4 || accounts.length > 3 ? <span className="adm-muted">...</span> : null}
    </span>
  );
}

function Json({ label, row }: { readonly label: string; readonly row: AuditRow }) {
  const text = JSON.stringify(row.raw, null, 2);
  return (
    <section className="adm-audit__json" aria-label={label}>
      <div className="adm-row">
        <h4 className="adm-h4 adm-grow">{label}</h4>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            if (copyText(text)) toast.success(T.detail.copied);
            else toast.error(new Error(T.detail.copyFailed));
          }}
        >
          {T.detail.copy}
        </Button>
      </div>
      <pre className="adm-audit__pre">{text}</pre>
    </section>
  );
}

export function RecordDetail({
  row,
  requested,
  missingOutcome,
  onClose,
}: {
  readonly row: AuditRow;
  /** The request record that belongs to `row`. */
  readonly requested?: AuditRow | null;
  /** `row` is a request that has no outcome. */
  readonly missingOutcome?: boolean;
  readonly onClose: () => void;
}) {
  return (
    <section className="adm-audit__detail" aria-label={T.detail.title}>
      <div className="adm-row">
        <h3 className="adm-h3 adm-grow">
          <code>{row.ev}</code> <span className="adm-muted">{formatStamp(row.ts)}</span>
        </h3>
        {row.state !== null && <Badge tone={stateTone(row.state)}>{row.state}</Badge>}
        <Button size="sm" variant="ghost" onClick={onClose}>
          {T.detail.close}
        </Button>
      </div>
      <dl className="adm-audit__facts">
        <dt>Actor</dt>
        <dd>{row.actor ?? '-'}</dd>
        <dt>Reason</dt>
        <dd>{row.reason ?? requested?.reason ?? '-'}</dd>
        <dt>Subject</dt>
        <dd>
          <SubjectLinks row={row} />
          {row.subject.serials.length > 0 && (
            <span className="adm-muted">{` serials ${row.subject.serials.slice(0, 6).join(', ')}`}</span>
          )}
        </dd>
        {row.rid !== null && (
          <>
            <dt>Request id</dt>
            <dd>
              <code>{row.rid}</code>
            </dd>
          </>
        )}
      </dl>
      {missingOutcome === true && <p className="adm-muted">{T.detail.noOutcome}</p>}
      {requested ? (
        <>
          <Json label={T.detail.outcome} row={row} />
          <Json label={T.detail.requested} row={requested} />
        </>
      ) : (
        <Json label={T.detail.raw} row={row} />
      )}
    </section>
  );
}
