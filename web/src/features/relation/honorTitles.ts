// The honor titles of the client (Plan/45 §24.3, Plan/21 §13; WP-CFEAT-40): the `honor` bundle as `CTitleManager` (`GetTitle`, `GetNeedCount`, `IsEarned`, `AddEarned`, `RemoveEarned`,
// `InitEarned`; `src:_Common/honor.cpp:264-365, 484-520`) and the two ways the earned list is built: silently from the counters of `SpawnSelf` (`ObjSerializeOpt.cpp:612-640`) and with the
// notices of `CDPClient::OnHonorListAck` (`DPClient.cpp:17370-17410`). Pure: no store, no window.

import type { HonorTitleRow } from '../../gen/bundles';

/** `MAX_HONOR_TITLE`: the number of counters on the wire (the index is the title id). */
export const MAX_HONOR_TITLE = 150;

/** `EarnedTitle`: the id and the text of a title the player has earned. */
export interface EarnedTitle {
  readonly id: number;
  readonly title: string;
}

/** The titles of the `honor` bundle by id. */
export class HonorTable {
  readonly #rows: ReadonlyMap<number, HonorTitleRow>;

  constructor(rows: readonly HonorTitleRow[]) {
    this.#rows = new Map(rows.map((r) => [r.id, r]));
  }

  get size(): number {
    return this.#rows.size;
  }

  /** `CTitleManager::GetTitle`: the text, `''` for `-1` and for an id that is not in the table (the original returned `NULL`). */
  titleOf(id: number): string {
    return this.#rows.get(id)?.title ?? '';
  }

  /** `GetNeedCount( id, -1 )`: the count a title needs, `-1` for an unknown id. */
  needOf(id: number): number {
    return this.#rows.get(id)?.need ?? -1;
  }

  /** The rule of both loops: a title is earned when it needs something (`0 < need`; title 0 "Energizer Bunny" needs 500) and the counter reached it. */
  isEarned(id: number, counter: number): boolean {
    const need = this.needOf(id);
    return need > 0 && counter >= need;
  }
}

/** `InitEarned` + `AddEarned` for every id that is earned, in ascending id order: the list of the join, built without a notice. */
export function earnedFromCounters(
  table: HonorTable,
  counters: readonly number[],
): readonly EarnedTitle[] {
  const out: EarnedTitle[] = [];
  const n = Math.min(counters.length, MAX_HONOR_TITLE);
  for (let id = 0; id < n; id += 1) {
    if (table.isEarned(id, counters[id] ?? 0)) out.push({ id, title: table.titleOf(id) });
  }
  return out;
}

export interface HonorListResult {
  /** The new list: the old order, the lost titles gone, the new ones appended in ascending id order (`push_back`). */
  readonly earned: readonly EarnedTitle[];
  /** The titles that were not earned before and are now: each gets `TID_GAME_GET_TITLE`. */
  readonly gained: readonly EarnedTitle[];
}

/**
 * `CDPClient::OnHonorListAck`: for every id of the 150 counters a title that reaches its need and is not on the list is appended (and announced); a title that is on the list and no longer
 * reaches it leaves the list. FIX: a list that is not exactly 150 long is the caller's to ignore (the original read 150 integers whatever came).
 */
export function applyHonorList(
  table: HonorTable,
  counters: readonly number[],
  before: readonly EarnedTitle[],
): HonorListResult {
  let earned = [...before];
  const gained: EarnedTitle[] = [];
  for (let id = 0; id < MAX_HONOR_TITLE; id += 1) {
    const has = earned.some((e) => e.id === id);
    if (table.isEarned(id, counters[id] ?? 0)) {
      if (!has) {
        const entry = { id, title: table.titleOf(id) };
        earned = [...earned, entry];
        gained.push(entry);
      }
    } else if (has) {
      earned = earned.filter((e) => e.id !== id);
    }
  }
  return { earned, gained };
}
