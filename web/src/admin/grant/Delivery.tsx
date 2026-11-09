// Delivery mode and reason (Plan/25 section 5.3, 12 rule 8): Auto sends online players the item at once and mails the rest; Direct needs
// them online with room; Pending waits for the next login; Mail sends a system mail with the subject and text given. The reason is
// mandatory and goes into the audit.
import { DEFAULT_REASON_PRESETS } from '../app/confirm';
import { ChipGroup, Field, TextInput, TextArea } from '../components';
import { useMeta } from './data';
import { useGrant, type ModeChoice } from './store';
import { T } from './strings';

/** The mail service cuts a subject at 31 bytes and a text at 255 (Plan/19 section 11.1). */
const MAIL_SUBJECT_MAX = 31;
const MAIL_TEXT_MAX = 255;

export function Delivery({ lockMode }: { readonly lockMode?: ModeChoice }) {
  const target = useGrant((s) => s.target);
  const mode = useGrant((s) => s.mode);
  const mailSubject = useGrant((s) => s.mailSubject);
  const mailText = useGrant((s) => s.mailText);
  const preset = useGrant((s) => s.preset);
  const note = useGrant((s) => s.note);
  const setMode = useGrant((s) => s.setMode);
  const setMail = useGrant((s) => s.setMail);
  const setReason = useGrant((s) => s.setReason);
  const pendingOk = useMeta((s) => s.meta?.features.pending_delivery ?? false);
  const modes: { id: ModeChoice; label: string; hint: string; disabled?: boolean }[] = [
    {
      id: 'auto',
      label: T.delivery.auto,
      hint: pendingOk ? T.delivery.autoHint : T.delivery.autoHintMail,
    },
    { id: 'direct', label: T.delivery.direct, hint: T.delivery.directHint },
    {
      id: 'pending',
      label: T.delivery.pending,
      hint: pendingOk ? T.delivery.pendingHint : T.delivery.pendingOff,
      disabled: !pendingOk,
    },
    { id: 'mail', label: T.delivery.mail, hint: T.delivery.mailHint },
  ];
  return (
    <section className="grant-delivery" aria-label={T.delivery.title}>
      {target === 'recipients' && lockMode === undefined && (
        <fieldset className="grant-modes">
          <legend className="grant-h">{T.delivery.title}</legend>
          {modes.map((m) => (
            <label key={m.id} className={`grant-mode ${m.disabled === true ? 'is-disabled' : ''}`}>
              <input
                type="radio"
                name="grant-mode"
                value={m.id}
                checked={mode === m.id}
                disabled={m.disabled === true}
                onChange={() => {
                  setMode(m.id);
                }}
              />
              <span>
                <strong>{m.label}</strong>
                <span className="grant-hint"> {m.hint}</span>
              </span>
            </label>
          ))}
        </fieldset>
      )}
      {target === 'recipients' && lockMode !== undefined && (
        <div>
          <h4 className="grant-h">{T.delivery.mail}</h4>
          <div className="grant-hint">{T.delivery.mailHint}</div>
        </div>
      )}
      {target === 'recipients' && mode === 'mail' && (
        <div className="grant-stack">
          <Field label={T.delivery.subject} required>
            <TextInput
              maxLength={MAIL_SUBJECT_MAX}
              value={mailSubject}
              onChange={(e) => {
                setMail(e.target.value, mailText);
              }}
            />
          </Field>
          <Field label={T.delivery.text}>
            <TextArea
              rows={2}
              maxLength={MAIL_TEXT_MAX}
              value={mailText}
              onChange={(e) => {
                setMail(mailSubject, e.target.value);
              }}
            />
          </Field>
        </div>
      )}
      <div className="grant-reason">
        <h4 className="grant-h">{T.delivery.reason}</h4>
        <ChipGroup
          label={T.delivery.reason}
          chips={DEFAULT_REASON_PRESETS.map((p) => ({ value: p, label: p }))}
          value={preset}
          onChange={(v) => {
            setReason(v, note);
          }}
        />
        <TextInput
          aria-label={T.delivery.note}
          placeholder={T.delivery.notePlaceholder}
          maxLength={200}
          value={note}
          onChange={(e) => {
            setReason(preset, e.target.value);
          }}
        />
      </div>
    </section>
  );
}
