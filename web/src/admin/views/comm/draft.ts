// The draft of a notice before it is sent or scheduled (Plan/25 section 8.1): what the form holds and the checks that need no server.
// The server checks everything again; this keeps the buttons honest and the messages near the fields. Pure.
import type {
  NoticeBody,
  NoticeRepeatDto,
  NoticeRow,
  NoticeStyleDto,
  NoticeTargetDto,
} from '../../gen/dto';
import { utf8Length } from './repeat';
import { T } from './strings';

/** The most characters one notice names (the server's `MAX_NOTICE_CHARACTERS`). */
export const MAX_CHARACTERS = 100;
/** The longest repeat interval, minutes (30 days). */
export const REPEAT_MAX_MIN = 43_200;

export interface PickedCharacter {
  readonly id: number;
  readonly name: string;
}

export type TargetDraft =
  | { readonly kind: 'all' }
  | { readonly kind: 'world'; readonly world: number | null }
  | { readonly kind: 'characters'; readonly characters: readonly PickedCharacter[] };

export interface Draft {
  readonly text: string;
  readonly style: NoticeStyleDto;
  readonly target: TargetDraft;
}

export const EMPTY_DRAFT: Draft = { text: '', style: 'system', target: { kind: 'all' } };

/** The wire audience of a draft, `null` while it is incomplete (no world picked, no character chosen). */
export function targetOf(t: TargetDraft): NoticeTargetDto | null {
  switch (t.kind) {
    case 'all':
      return { kind: 'all' };
    case 'world':
      return t.world === null ? null : { kind: 'world', world: t.world };
    case 'characters':
      return t.characters.length === 0
        ? null
        : { kind: 'characters', characters: t.characters.map((c) => c.id) };
  }
}

/** The longest text of a style, bytes (from the list answer; the wire limits until it arrives). */
export interface Limits {
  readonly system: number;
  readonly caption: number;
}

export const DEFAULT_LIMITS: Limits = { system: 511, caption: 260 };

export function maxBytes(style: NoticeStyleDto, limits: Limits): number {
  return style === 'caption' ? limits.caption : limits.system;
}

/** Why the text cannot go out (`null` when it can). */
export function textProblem(text: string, style: NoticeStyleDto, limits: Limits): string | null {
  const t = text.trim();
  if (t === '') return null;
  const max = maxBytes(style, limits);
  const used = utf8Length(t);
  if (used > max) return T.notices.tooLong(used, max);
  // eslint-disable-next-line no-control-regex -- the game refuses control characters
  if (/[\u0000-\u001f\u007f]/.test(t)) return 'No control characters.';
  return null;
}

/** `true` when the draft can be sent. */
export function sendable(d: Draft, limits: Limits): boolean {
  return (
    d.text.trim() !== '' &&
    textProblem(d.text, d.style, limits) === null &&
    targetOf(d.target) !== null &&
    (d.target.kind !== 'characters' || d.target.characters.length <= MAX_CHARACTERS)
  );
}

/** The draft of an existing row (an edit). */
export function draftOfRow(row: NoticeRow, names: ReadonlyMap<number, string> = new Map()): Draft {
  const t = row.target;
  return {
    text: row.text,
    style: row.style,
    target:
      t.kind === 'all'
        ? { kind: 'all' }
        : t.kind === 'world'
          ? { kind: 'world', world: t.world }
          : {
              kind: 'characters',
              characters: t.characters.map((id) => ({
                id,
                name: names.get(id) ?? `#${String(id)}`,
              })),
            },
  };
}

/** The schedule part of the form. Times are strings of `<input type="datetime-local">` in server time. */
export interface ScheduleDraft {
  readonly start: string;
  readonly repeat: NoticeRepeatDto['kind'];
  readonly minutes: string;
  readonly at: string;
  readonly end: string;
  readonly paused: boolean;
}

export const HHMM = /^([01]\d|2[0-3]):[0-5]\d$/;

export interface ScheduleCheck {
  readonly problems: Readonly<
    Partial<Record<'start' | 'end' | 'minutes' | 'at' | 'text' | 'target', string>>
  >;
  readonly body: NoticeBody | null;
}

/** Checks a notice and its schedule and builds the body. `toUnix` reads a datetime-local value in server time. */
export function checkSchedule(
  d: Draft,
  s: ScheduleDraft,
  limits: Limits,
  toUnix: (value: string) => number | null,
  nowS: number,
  keepsPastStart = false,
): ScheduleCheck {
  const problems: Record<string, string> = {};
  if (d.text.trim() === '') problems['text'] = T.notices.text;
  const tp = textProblem(d.text, d.style, limits);
  if (tp !== null) problems['text'] = tp;
  const target = targetOf(d.target);
  if (target === null) {
    problems['target'] = d.target.kind === 'characters' ? T.notices.chooseOne : T.notices.pickWorld;
  }
  const start = toUnix(s.start);
  if (start === null) problems['start'] = T.notices.badStart;
  const hasEnd = s.repeat !== 'once' && s.end.trim() !== '';
  const end = hasEnd ? toUnix(s.end) : null;
  if (hasEnd && end === null) problems['end'] = T.notices.badEnd;
  if (start !== null && end !== null && end <= start) problems['end'] = T.notices.badEnd;
  let repeat: NoticeRepeatDto = { kind: 'once' };
  if (s.repeat === 'every') {
    const n = Number(s.minutes);
    if (!Number.isInteger(n) || n < 1 || n > REPEAT_MAX_MIN) {
      problems['minutes'] = T.notices.badMinutes(REPEAT_MAX_MIN);
    } else {
      repeat = { kind: 'every', minutes: n };
    }
  } else if (s.repeat === 'daily') {
    if (HHMM.test(s.at)) repeat = { kind: 'daily', at: s.at };
    else problems['at'] = T.notices.badTime;
  } else if (start !== null && start < nowS - 600 && !keepsPastStart) {
    problems['start'] = T.notices.pastOnce;
  }
  if (Object.keys(problems).length > 0 || start === null || target === null) {
    return { problems, body: null };
  }
  return {
    problems,
    body: {
      text: d.text.trim(),
      style: d.style,
      target,
      start_at: start,
      repeat,
      end_at: end,
      paused: s.paused,
    },
  };
}
