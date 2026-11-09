// ConfirmDialog (Plan/25 section 2.3): the simple confirmation (one click), the typed confirmation (type the target name, or
// CONFIRM), reason presets with free text, and duration chips for mute and ban. The confirm button stays disabled until
// everything the dialog asks for is given. The caller's `onConfirm` may reject: the message shows in the dialog and it stays open.
import { useState, type ReactNode } from 'react';
import { confirmationMatches, DEFAULT_REASON_PRESETS, type DurationChip } from '../app/confirm';
import type { Reason } from '../gen/dto';
import { Button } from './Button';
import { Dialog } from './Dialog';
import { ChipGroup, Field, TextArea, TextInput } from './fields';

export interface ConfirmResult {
  readonly reason?: Reason;
  /** The typed text (the target name or `CONFIRM`), for a typed confirmation. */
  readonly confirm?: string;
  readonly duration?: DurationChip | null;
}

export interface ConfirmDialogProps {
  readonly title: string;
  readonly message?: ReactNode;
  readonly confirmLabel?: string;
  /** The confirm button is red (destructive actions). */
  readonly danger?: boolean;
  readonly level: 'simple' | 'typed';
  /** For `typed`: what must be typed (the target name, or `CONFIRM`). */
  readonly expected?: string;
  readonly reason?: 'required' | 'optional' | 'none';
  readonly presets?: readonly string[];
  /** Duration chips (mute, ban); `durationRequired` makes a choice mandatory. */
  readonly durations?: readonly DurationChip[];
  readonly durationRequired?: boolean;
  /** Chips that start chosen (the last choice of the operator, a sensible default): one click on the confirm button then finishes the action. */
  readonly initialPreset?: string;
  readonly initialDuration?: string;
  readonly onConfirm: (result: ConfirmResult) => void | Promise<void>;
  readonly onCancel: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel = 'Confirm',
  danger = false,
  level,
  expected = 'CONFIRM',
  reason = 'none',
  presets = DEFAULT_REASON_PRESETS,
  durations,
  durationRequired = false,
  initialPreset,
  initialDuration,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const [typed, setTyped] = useState('');
  const [preset, setPreset] = useState<string | null>(
    initialPreset !== undefined && presets.includes(initialPreset) ? initialPreset : null,
  );
  const [text, setText] = useState('');
  const [duration, setDuration] = useState<string | null>(
    initialDuration !== undefined && durations?.some((d) => d.id === initialDuration)
      ? initialDuration
      : null,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const typedOk = level !== 'typed' || confirmationMatches(typed, expected);
  const reasonOk = reason !== 'required' || preset !== null || text.trim() !== '';
  const durationOk = !durationRequired || duration !== null;
  const ready = typedOk && reasonOk && durationOk;

  const submit = async (): Promise<void> => {
    if (!ready || busy) return;
    const chosen = durations?.find((d) => d.id === duration) ?? null;
    const result: ConfirmResult = {
      ...(reason !== 'none' && (preset !== null || text.trim() !== '')
        ? {
            reason: {
              ...(preset !== null ? { preset } : {}),
              ...(text.trim() !== '' ? { text: text.trim() } : {}),
            },
          }
        : {}),
      ...(level === 'typed' ? { confirm: typed.trim() } : {}),
      ...(durations ? { duration: chosen } : {}),
    };
    setBusy(true);
    setError(null);
    try {
      await onConfirm(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={title}
      onClose={onCancel}
      busy={busy}
      footer={
        <>
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant={danger ? 'danger' : 'primary'}
            disabled={!ready}
            loading={busy}
            onClick={() => void submit()}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      <form
        className="adm-stack"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        {message ? <div>{message}</div> : null}
        {durations ? (
          <div className="adm-stack adm-stack--tight">
            <span className="adm-field__label">Duration{durationRequired ? ' *' : ''}</span>
            <ChipGroup
              label="Duration"
              chips={durations.map((d) => ({ value: d.id, label: d.label }))}
              value={duration}
              onChange={setDuration}
            />
          </div>
        ) : null}
        {reason !== 'none' ? (
          <div className="adm-stack adm-stack--tight">
            <span className="adm-field__label">Reason{reason === 'required' ? ' *' : ''}</span>
            <ChipGroup
              label="Reason"
              chips={presets.map((p) => ({ value: p, label: p }))}
              value={preset}
              onChange={setPreset}
            />
            <Field label="Details (optional)">
              <TextArea
                rows={2}
                maxLength={500}
                value={text}
                onChange={(e) => {
                  setText(e.target.value);
                }}
              />
            </Field>
          </div>
        ) : null}
        {level === 'typed' ? (
          <Field
            label={
              <>
                Type <code>{expected}</code> to confirm
              </>
            }
            hint={
              expected === 'CONFIRM'
                ? 'This affects several targets or cannot be undone.'
                : 'This cannot be undone.'
            }
          >
            <TextInput
              data-autofocus
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              value={typed}
              onChange={(e) => {
                setTyped(e.target.value);
              }}
            />
          </Field>
        ) : null}
        {error ? (
          <div className="adm-alert adm-alert--danger" role="alert">
            {error}
          </div>
        ) : null}
        {/* a submit button inside the form so Enter confirms once everything is given */}
        <button type="submit" hidden disabled={!ready} />
      </form>
    </Dialog>
  );
}
