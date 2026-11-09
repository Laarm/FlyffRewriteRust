// The sorting of the guild member page (`src:_Interface/WndGuildTabMember.cpp:30-165, 740-880`, `__CSC_VER12_2`; Plan/42 §19.4 J; WP-UI-45): the ten
// comparators of the original ported exactly, and the state machine of the five sort buttons (`WIDC_MLEVEL`, `WIDC_JOB`, `WIDC_LEVEL`, `WIDC_NAME`,
// `WIDC_ALIAS`). The names of the originals are inverted (`prJobAsce( a, b )` is `a` before `b` when `a` has the GREATER job type, so it sorts downwards);
// the functions here keep the names and the bodies. A comparator answers "a goes before b". `std::sort` is unstable: this port is a stable sort over rows
// in character id order, so ties come out by character id (the task ruling).
//
// The state (`m_bSortbyMemberLevel` starts FALSE, the other four TRUE): a click uses the "Asce" comparator when the flag is set (and clears it), else the
// "Desc" one (and sets it). The default sort at the opening is a click on the member-level button: flag FALSE -> "Desc" = the lowest member level first, the
// master on top; its class ties come out highest class first. With one member or none (`m_nMax > 1` fails) nothing is sorted and no flag changes.
// A rebuild of the list (the original left it in id order until the next click; here it is sorted again) re-applies the direction of the last click.

import type { GuildMemberRow } from './ports';

export type MemberSortKey = 'mlevel' | 'job' | 'level' | 'name' | 'alias';
export type SortDirection = 'asce' | 'desc';

/** The fields of a row the comparators read (`MEMBERLIST`). */
export type MemberSortRow = Pick<
  GuildMemberRow,
  'rank' | 'cls' | 'jobType' | 'job' | 'level' | 'name' | 'alias'
>;

/** `a` goes before `b`. */
export type Before = (a: MemberSortRow, b: MemberSortRow) => boolean;

/** `strcmp` on the bytes of the text (`CString::operator >`, `operator <`). */
export function compareBytes(a: string, b: string): number {
  const x = new TextEncoder().encode(a);
  const y = new TextEncoder().encode(b);
  const n = Math.min(x.length, y.length);
  for (let i = 0; i < n; i += 1) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d !== 0) return d;
  }
  return x.length - y.length;
}

export const prMemberLevelAsce: Before = (a, b) =>
  a.rank > b.rank || (a.rank === b.rank && a.cls < b.cls);
export const prMemberLevelDesc: Before = (a, b) =>
  a.rank < b.rank || (a.rank === b.rank && a.cls > b.cls);
export const prJobAsce: Before = (a, b) =>
  a.jobType > b.jobType || (a.jobType === b.jobType && a.job > b.job);
export const prJobDesc: Before = (a, b) =>
  a.jobType < b.jobType || (a.jobType === b.jobType && a.job < b.job);
export const prLevelAsce: Before = (a, b) =>
  a.jobType > b.jobType || (a.jobType === b.jobType && a.level > b.level);
export const prLevelDesc: Before = (a, b) =>
  a.jobType < b.jobType || (a.jobType === b.jobType && a.level < b.level);
export const prNameAsce: Before = (a, b) => compareBytes(a.name, b.name) > 0;
export const prNameDesc: Before = (a, b) => compareBytes(a.name, b.name) < 0;
export const prAliasAsce: Before = (a, b) => compareBytes(a.alias, b.alias) > 0;
export const prAliasDesc: Before = (a, b) => compareBytes(a.alias, b.alias) < 0;

/** The comparator of a button in a direction. */
export const MEMBER_COMPARATORS: Readonly<
  Record<MemberSortKey, Readonly<Record<SortDirection, Before>>>
> = {
  mlevel: { asce: prMemberLevelAsce, desc: prMemberLevelDesc },
  job: { asce: prJobAsce, desc: prJobDesc },
  level: { asce: prLevelAsce, desc: prLevelDesc },
  name: { asce: prNameAsce, desc: prNameDesc },
  alias: { asce: prAliasAsce, desc: prAliasDesc },
};

/** A stable sort by a "goes before" comparator. */
export function sortMembers<R extends MemberSortRow>(rows: readonly R[], before: Before): R[] {
  const indexed = rows.map((row, index) => ({ row, index }));
  indexed.sort((x, y) => {
    if (before(x.row, y.row)) return -1;
    if (before(y.row, x.row)) return 1;
    return x.index - y.index;
  });
  return indexed.map((e) => e.row);
}

/** The five flags and the current sort of the member page. */
export class MemberListSort {
  #flags: Record<MemberSortKey, boolean> = {
    mlevel: false,
    job: true,
    level: true,
    name: true,
    alias: true,
  };
  #current: MemberSortKey | null = null;
  #dir: SortDirection = 'desc';

  /** The button of the last sort, `null` before the first one. */
  get current(): MemberSortKey | null {
    return this.#current;
  }
  /** The direction of the last sort. */
  get direction(): SortDirection {
    return this.#dir;
  }
  /** The flag of a button: `true` = the next click sorts "Asce". */
  flag(key: MemberSortKey): boolean {
    return this.#flags[key];
  }

  /** A click on a sort button (`SortbyX()`): sorts and flips the flag; one row or none changes nothing. */
  sortClick<R extends MemberSortRow>(rows: readonly R[], key: MemberSortKey): R[] {
    if (rows.length <= 1) return rows.slice();
    const asce = this.#flags[key];
    this.#flags[key] = !asce;
    this.#current = key;
    this.#dir = asce ? 'asce' : 'desc';
    return sortMembers(rows, MEMBER_COMPARATORS[key][this.#dir]);
  }

  /** A rebuild of the list: the sort of the last click again (the default one - the member level - before any click). */
  sortRebuild<R extends MemberSortRow>(rows: readonly R[]): R[] {
    if (this.#current === null) return this.sortClick(rows, 'mlevel');
    if (rows.length <= 1) return rows.slice();
    return sortMembers(rows, MEMBER_COMPARATORS[this.#current][this.#dir]);
  }
}
