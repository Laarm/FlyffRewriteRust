// The dialog of a scheduled or recurring notice (Plan/25 section 8.1): text, look and audience as for a notice now, the first time, how it
// repeats (once, every N minutes, daily at hh:mm), an optional end and the pause switch. Times are server time.
import { useMemo, useState } from 'react';
import { Button, Checkbox, Dialog, Field, NumberInput, Select, TextInput } from '../../components';
import type { NoticeRepeatDto, NoticeRow } from '../../gen/dto';
import { toast } from '../../stores/toasts';
import { createNotice, updateNotice } from './api';
import {
  checkSchedule,
  draftOfRow,
  EMPTY_DRAFT,
  type Draft,
  type Limits,
  type ScheduleDraft,
} from './draft';
import { NoticeFields } from './NoticeFields';
import { T } from './strings';
import { fromInputValue, toInputValue } from './zone';

const N = T.notices;

const REPEATS: readonly NoticeRepeatDto['kind'][] = ['once', 'every', 'daily'];

/** The first time offered for a new notice: a quarter hour ahead on the next quarter, in server time. */
export function defaultStart(nowS: number, zone: string): string {
  const next = Math.ceil((nowS + 600) / 900) * 900;
  return toInputValue(next, zone);
}

function scheduleOfRow(row: NoticeRow, zone: string): ScheduleDraft {
  return {
    start: toInputValue(row.start_at, zone),
    repeat: row.repeat.kind,
    minutes: row.repeat.kind === 'every' ? String(row.repeat.minutes) : '60',
    at: row.repeat.kind === 'daily' ? row.repeat.at : '18:00',
    end: row.end_at === null ? '' : toInputValue(row.end_at, zone),
    paused: row.paused,
  };
}

export interface ScheduleDialogProps {
  /** The notice to edit; `null` for a new one. */
  readonly row: NoticeRow | null;
  readonly zone: string;
  readonly limits: Limits;
  readonly nowS: number;
  readonly onSaved: () => void;
  readonly close: () => void;
}

export function ScheduleDialog({ row, zone, limits, nowS, onSaved, close }: ScheduleDialogProps) {
  const [draft, setDraft] = useState<Draft>(() => (row ? draftOfRow(row) : EMPTY_DRAFT));
  const [schedule, setSchedule] = useState<ScheduleDraft>(() =>
    row
      ? scheduleOfRow(row, zone)
      : {
          start: defaultStart(nowS, zone),
          repeat: 'once',
          minutes: '60',
          at: '18:00',
          end: '',
          paused: false,
        },
  );
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [shown, setShown] = useState(false);
  const check = useMemo(
    () =>
      checkSchedule(draft, schedule, limits, (v) => fromInputValue(v, zone), nowS, row !== null),
    [draft, schedule, limits, zone, nowS, row],
  );
  const problem = (key: keyof typeof check.problems): string | undefined =>
    shown ? check.problems[key] : undefined;

  const save = async (): Promise<void> => {
    setShown(true);
    if (check.body === null) return;
    setBusy(true);
    setFailure(null);
    try {
      if (row) await updateNotice(row.id, check.body);
      else await createNotice(check.body);
      toast.success(row ? N.saved : N.created);
      onSaved();
      close();
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error));
      setBusy(false);
    }
  };

  const set = (patch: Partial<ScheduleDraft>): void => {
    setSchedule({ ...schedule, ...patch });
  };

  return (
    <Dialog
      title={row ? N.formEdit : N.formNew}
      size="lg"
      busy={busy}
      onClose={close}
      footer={
        <>
          <Button onClick={close} disabled={busy}>
            {N.cancel}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            onClick={() => {
              void save();
            }}
          >
            {N.save}
          </Button>
        </>
      }
    >
      <div className="comm-form">
        <NoticeFields
          draft={draft}
          onChange={setDraft}
          limits={limits}
          errors={{ text: problem('text'), target: problem('target') }}
        />
        <div className="comm-row">
          <Field label={N.start} hint={N.startHint} error={problem('start')} required>
            <TextInput
              type="datetime-local"
              value={schedule.start}
              onChange={(e) => {
                set({ start: e.target.value });
              }}
            />
          </Field>
          <Field label={N.repeat}>
            <Select
              value={schedule.repeat}
              onChange={(e) => {
                set({ repeat: REPEATS.find((r) => r === e.target.value) ?? 'once' });
              }}
            >
              {REPEATS.map((r) => (
                <option key={r} value={r}>
                  {N.repeats[r]}
                </option>
              ))}
            </Select>
          </Field>
          {schedule.repeat === 'every' && (
            <Field label={N.minutes} error={problem('minutes')}>
              <NumberInput
                min={1}
                value={schedule.minutes}
                onChange={(e) => {
                  set({ minutes: e.target.value });
                }}
              />
            </Field>
          )}
          {schedule.repeat === 'daily' && (
            <Field label={N.dailyAt} error={problem('at')}>
              <TextInput
                type="time"
                value={schedule.at}
                onChange={(e) => {
                  set({ at: e.target.value });
                }}
              />
            </Field>
          )}
          {schedule.repeat !== 'once' && (
            <Field label={N.end} hint={N.endHint} error={problem('end')}>
              <TextInput
                type="datetime-local"
                value={schedule.end}
                onChange={(e) => {
                  set({ end: e.target.value });
                }}
              />
            </Field>
          )}
        </div>
        <Checkbox
          label={N.paused}
          checked={schedule.paused}
          onChange={(e) => {
            set({ paused: e.target.checked });
          }}
        />
        {failure !== null && (
          <div className="adm-field__error" role="alert">
            {failure}
          </div>
        )}
      </div>
    </Dialog>
  );
}
