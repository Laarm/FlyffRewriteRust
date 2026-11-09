// The grant UI (Plan/25 section 5.3): catalogue, builder with the live tooltip, basket, recipients, delivery and reason. ONE component:
// the Item builder tab shows it full width and the grant dialog shows it in a wide modal, both over their own store.
import { useEffect } from 'react';
import { Badge, Button, Field, TextInput } from '../components';
import './grant.css';
import { confirmationMatches } from '../app/confirm';
import { Basket } from './Basket';
import { Builder } from './Builder';
import { Catalogue } from './Catalogue';
import { Delivery } from './Delivery';
import { ensureGrantData, ensureMeta, useGrantData } from './data';
import { Recipients } from './Recipients';
import { recipientsText } from './spec';
import { submitDrop, submitGrant, summarize } from './submit';
import {
  basketTotals,
  blockedReason,
  type ModeChoice,
  recipientsOf,
  useGrant,
  useGrantStoreApi,
} from './store';
import { T } from './strings';

function Report() {
  const report = useGrant((s) => s.report);
  if (!report) return null;
  const sum = summarize(report);
  return (
    <div className="grant-report" role="status" data-testid="grant-report">
      <div className="grant-report__line">
        {sum.delivered > 0 && <Badge tone="ok">{T.report.delivered(sum.delivered)}</Badge>}
        {sum.mailed > 0 && <Badge tone="accent">{T.report.mailed(sum.mailed)}</Badge>}
        {sum.pending > 0 && <Badge tone="accent">{T.report.pending(sum.pending)}</Badge>}
        {sum.failed.length > 0 && <Badge tone="danger">{T.report.failed(sum.failed.length)}</Badge>}
      </div>
      {sum.failed.map((f) => (
        <div key={f.id} className="adm-field__error">
          {f.name}: {f.message}
        </div>
      ))}
      <div className="grant-hint">{T.report.request(report.request_id)}</div>
    </div>
  );
}

function SubmitBar() {
  const api = useGrantStoreApi();
  const s = useGrant((x) => x);
  const totals = basketTotals(s);
  const blocked = blockedReason(s);
  const ground = s.target === 'ground';
  const typed = s.confirmFor === null || confirmationMatches(s.confirm, s.confirmFor);
  const who = ground ? T.recipients.ground : recipientsText(recipientsOf(s));
  return (
    <section className="grant-submit" aria-label={T.panel.submit}>
      <div className="grant-summary">
        {T.panel.summary(totals.lines, totals.pieces, totals.gold, who)}
      </div>
      {s.confirmFor !== null && (
        <Field label={T.panel.typeToConfirm(s.confirmFor)} required>
          <TextInput
            value={s.confirm}
            autoComplete="off"
            data-autofocus
            onChange={(e) => {
              s.setConfirm(e.target.value);
            }}
          />
        </Field>
      )}
      {s.error && (
        <div className="adm-field__error" role="alert">
          {s.error}
        </div>
      )}
      {s.dropped && (
        <div className="grant-note" role="status">
          {s.dropped}
        </div>
      )}
      {blocked !== null && (
        <div className="grant-hint">{T.blocked[blocked as keyof typeof T.blocked]}</div>
      )}
      <Button
        variant="primary"
        loading={s.busy}
        disabled={blocked !== null || !typed}
        onClick={() => {
          if (ground) void submitDrop(api);
          else void submitGrant(api);
        }}
      >
        {ground ? T.panel.drop : T.panel.grant}
      </Button>
      <Report />
    </section>
  );
}

/**
 * `lockMode` fixes the delivery mode: the radio buttons are not shown (the System mail tab is the panel in Mail mode, Plan/25 section 8.2).
 * The store must already be in that mode (`createGrantStore({ mode })`).
 */
export function GrantPanel({
  variant,
  lockMode,
}: {
  readonly variant: 'tab' | 'dialog';
  readonly lockMode?: ModeChoice;
}) {
  const phase = useGrantData((s) => s.phase);
  useEffect(() => {
    void ensureGrantData();
    void ensureMeta();
  }, []);
  return (
    <div
      className={`grant-panel grant-panel--${variant}`}
      data-testid="grant-panel"
      data-data-phase={phase}
    >
      <Catalogue />
      <Builder />
      <div className="grant-col grant-side">
        <Basket />
        <Recipients />
        <Delivery {...(lockMode === undefined ? {} : { lockMode })} />
        <SubmitBar />
      </div>
    </div>
  );
}
