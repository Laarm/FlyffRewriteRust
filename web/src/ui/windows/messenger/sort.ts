// The sorting of the messenger lists (`src:_Interface/WndMessengerCtrl.cpp:28-226, 966-1056`, Plan/42 §19.4 I; WP-UI-43): the ten comparators of the
// original ported exactly, and the state machine of the five sort buttons.
//
// The names of the originals are inverted (`prLevelAsce( a, b )` is `a` before `b` when `a` has the GREATER job type, so it sorts downwards); the
// functions here keep the names and the bodies, `std::sort` becomes a stable sort. A comparator answers "a goes before b".
//
// The state machine (`CWndFriendCtrlEx::SortbyX( bCheckbefore )`): each button has a flag. A click (`bCheckbefore = TRUE`) uses the "Asce"
// comparator when the flag is set (and clears it), else the "Desc" one (and sets it). A rebuild of the list (`UpdatePlayerList` calls
// `SortbyX( FALSE )`) flips the flag first and then does the same, which re-applies the direction of the last click and leaves the flag as it was.
// Start: status is the current sort with its flag clear, the other four flags are set - so the first rebuild sorts by status "Asce" (online
// first), and the first click on channel, level, job or name is "Asce", on status "Desc".

import { FRS, type MessengerRow } from './ports';

export type SortKey = 'channel' | 'status' | 'level' | 'job' | 'name';
export type SortDirection = 'asce' | 'desc';

/** `a` goes before `b`. */
export type Before = (a: SortRow, b: SortRow) => boolean;

/** The fields of a row the comparators read (`__MESSENGER_PLAYER`). */
export type SortRow = Pick<
  MessengerRow,
  'channel' | 'state' | 'level' | 'job' | 'jobType' | 'name'
>;

/** `strcmp` on the bytes of the names (`CString::operator >`). */
export function compareNames(a: string, b: string): number {
  const x = new TextEncoder().encode(a);
  const y = new TextEncoder().encode(b);
  const n = Math.min(x.length, y.length);
  for (let i = 0; i < n; i += 1) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d !== 0) return d;
  }
  return x.length - y.length;
}

/** `m_dwStatus` with `FRS_OFFLINE` counted as 12 ("the offline state is the last"). */
const statusKey = (r: SortRow): number => (r.state === FRS.OFFLINE ? 12 : r.state);

export const prLevelAsce: Before = (a, b) =>
  a.jobType > b.jobType || (a.jobType === b.jobType && a.level > b.level);
export const prLevelDesc: Before = (a, b) =>
  a.jobType < b.jobType || (a.jobType === b.jobType && a.level < b.level);
export const prJobAsce: Before = (a, b) =>
  a.jobType > b.jobType || (a.jobType === b.jobType && a.job > b.job);
export const prJobDesc: Before = (a, b) =>
  a.jobType < b.jobType || (a.jobType === b.jobType && a.job < b.job);
export const prStatusAsce: Before = (a, b) => statusKey(a) < statusKey(b);
export const prStatusDesc: Before = (a, b) => statusKey(a) > statusKey(b);
export const prChannelAsce: Before = (a, b) => a.channel > b.channel;
export const prChannelDesc: Before = (a, b) => a.channel < b.channel;
export const prNameAsce: Before = (a, b) => compareNames(a.name, b.name) > 0;
export const prNameDesc: Before = (a, b) => compareNames(a.name, b.name) < 0;

/** The comparator of a button in a direction. */
export const COMPARATORS: Readonly<Record<SortKey, Readonly<Record<SortDirection, Before>>>> = {
  channel: { asce: prChannelAsce, desc: prChannelDesc },
  status: { asce: prStatusAsce, desc: prStatusDesc },
  level: { asce: prLevelAsce, desc: prLevelDesc },
  job: { asce: prJobAsce, desc: prJobDesc },
  name: { asce: prNameAsce, desc: prNameDesc },
};

/** A stable sort by a "goes before" comparator (the original's `std::sort` leaves ties unspecified). */
export function sortBy<R extends SortRow>(rows: readonly R[], before: Before): R[] {
  const indexed = rows.map((row, index) => ({ row, index }));
  indexed.sort((x, y) => {
    if (before(x.row, y.row)) return -1;
    if (before(y.row, x.row)) return 1;
    return x.index - y.index;
  });
  return indexed.map((e) => e.row);
}

/** The five flags and the current sort of one list page. */
export class ListSort {
  #flags: Record<SortKey, boolean> = {
    channel: true,
    status: false,
    level: true,
    job: true,
    name: true,
  };
  #current: SortKey = 'status';

  /** The sort of the last click or rebuild. */
  get current(): SortKey {
    return this.#current;
  }

  /** The flag of a button: `true` = the next click sorts "Asce". */
  flag(key: SortKey): boolean {
    return this.#flags[key];
  }

  /** A click on a sort button: the direction it applies. */
  click(key: SortKey): SortDirection {
    return this.#apply(key, true);
  }

  /** A rebuild of the list: the current sort again, in the direction of the last click. */
  rebuild(): SortDirection {
    return this.#apply(this.#current, false);
  }

  #apply(key: SortKey, checkBefore: boolean): SortDirection {
    if (!checkBefore) this.#flags[key] = !this.#flags[key];
    const asce = this.#flags[key];
    this.#flags[key] = !asce;
    this.#current = key;
    return asce ? 'asce' : 'desc';
  }

  /** Sorts for a click. */
  sortClick<R extends SortRow>(rows: readonly R[], key: SortKey): R[] {
    return sortBy(rows, COMPARATORS[key][this.click(key)]);
  }

  /** Sorts for a rebuild. */
  sortRebuild<R extends SortRow>(rows: readonly R[]): R[] {
    const key = this.#current;
    return sortBy(rows, COMPARATORS[key][this.rebuild()]);
  }
}
