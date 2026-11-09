// Running a mutation with the confirmation it needs (Plan/25 section 2.3, 12 rules 8): `runConfirmed` opens the right dialog
// (none: runs at once; simple: one click; typed: type the target name or CONFIRM; with reason presets and duration chips where the
// action asks for them), calls `run` with what was given, and ends with a toast. The server recomputes every rule: when it answers
// `confirm_required` or `reason_required` for something the client did not ask for (the live thresholds differ from the defaults),
// the dialog is asked again with the missing part, so the operator is never stuck behind a bare error.
import type { ReactNode } from 'react';
import { ConfirmDialog, type ConfirmResult } from '../components/ConfirmDialog';
import { isApiError } from '../api/rest';
import { openDialog } from '../stores/dialogs';
import { toast } from '../stores/toasts';
import {
  confirmLevel,
  expectedConfirmation,
  expectedFromMessage,
  type ConfirmFacts,
  type DurationChip,
} from './confirm';

export interface ConfirmedOptions<T> {
  readonly title: string;
  readonly message?: ReactNode;
  readonly confirmLabel?: string;
  readonly danger?: boolean;
  readonly facts?: ConfirmFacts;
  /** Is a reason needed (kick, mute, ban, removals, grants, disbanding)? */
  readonly reason?: 'required' | 'optional' | 'none';
  readonly presets?: readonly string[];
  readonly durations?: readonly DurationChip[];
  readonly durationRequired?: boolean;
  /** The reason preset and the duration chip (its `id`) that start chosen. */
  readonly initial?: { readonly preset?: string; readonly duration?: string };
  /** Calls the API with what the dialog collected. */
  readonly run: (given: ConfirmResult) => Promise<T>;
  /** The success toast (nothing when omitted). */
  readonly success?: string | ((value: T) => string);
  /** The error toast prefix when `run` fails with the dialog closed (a `none` level). */
  readonly failure?: string;
}

export type Outcome<T> = { readonly ok: true; readonly value: T } | { readonly ok: false };

type Ask<T> =
  | { readonly status: 'done'; readonly value: T }
  | { readonly status: 'cancel' }
  | {
      readonly status: 'escalate';
      readonly level?: 'typed';
      readonly expected?: string;
      readonly reason?: 'required';
    };

/** Opens a confirmation, runs the mutation, toasts the result. Resolves `{ok:false}` when the operator cancelled or it failed. */
export async function runConfirmed<T>(options: ConfirmedOptions<T>): Promise<Outcome<T>> {
  const facts = options.facts ?? {};
  let level = confirmLevel(facts);
  let expected = expectedConfirmation(facts);
  let reason = options.reason ?? 'none';

  const succeeded = (value: T): Outcome<T> => {
    const { success } = options;
    if (success !== undefined)
      toast.success(typeof success === 'function' ? success(value) : success);
    return { ok: true, value };
  };

  if (level === 'none' && reason === 'none' && options.durations === undefined) {
    try {
      return succeeded(await options.run({}));
    } catch (error) {
      toast.error(error, options.failure ?? options.title);
      return { ok: false };
    }
  }
  if (level === 'none') level = 'simple';

  for (;;) {
    const lvl = level;
    const exp = expected;
    const rsn = reason;
    const answer = await new Promise<Ask<T>>((resolve) => {
      openDialog((close) => (
        <ConfirmDialog
          title={options.title}
          {...(options.message === undefined ? {} : { message: options.message })}
          {...(options.confirmLabel === undefined ? {} : { confirmLabel: options.confirmLabel })}
          {...(options.danger === undefined ? {} : { danger: options.danger })}
          level={lvl === 'typed' ? 'typed' : 'simple'}
          expected={exp}
          reason={rsn}
          {...(options.presets === undefined ? {} : { presets: options.presets })}
          {...(options.durations === undefined ? {} : { durations: options.durations })}
          {...(options.durationRequired === undefined
            ? {}
            : { durationRequired: options.durationRequired })}
          {...(options.initial?.preset === undefined
            ? {}
            : { initialPreset: options.initial.preset })}
          {...(options.initial?.duration === undefined
            ? {}
            : { initialDuration: options.initial.duration })}
          onCancel={() => {
            close();
            resolve({ status: 'cancel' });
          }}
          onConfirm={async (given) => {
            try {
              const value = await options.run(given);
              close();
              resolve({ status: 'done', value });
            } catch (error) {
              if (isApiError(error, 'confirm_required') && lvl !== 'typed') {
                close();
                resolve({
                  status: 'escalate',
                  level: 'typed',
                  expected: expectedFromMessage(error.message) ?? 'CONFIRM',
                });
                return;
              }
              if (isApiError(error, 'reason_required') && rsn !== 'required') {
                close();
                resolve({ status: 'escalate', reason: 'required' });
                return;
              }
              throw error; // the dialog shows it and stays open
            }
          }}
        />
      ));
    });
    if (answer.status === 'done') return succeeded(answer.value);
    if (answer.status === 'cancel') return { ok: false };
    if (answer.level) level = answer.level;
    if (answer.expected) expected = answer.expected;
    if (answer.reason) reason = answer.reason;
  }
}
