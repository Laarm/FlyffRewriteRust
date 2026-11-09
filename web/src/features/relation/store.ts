// `relationStore` (Plan/45 §24.2, §24.3; WP-CFEAT-40): the couple, the campus and its points, the honor titles and the cheer points of the own character - the mirror of
// `CCoupleHelper::m_pCouple`, `CCampusHelper::m_pCampus`, `g_pPlayer->m_nCampusPoint`, `CTitleManager::m_vecEarned` + `g_pPlayer->m_nHonor` and `g_pPlayer->m_nCheerPoint/m_dwTickCheer`.
// Names, levels and jobs of the partner and the campus members never live here: `features/common/playerData.ts` has them. Scope `session`: the join batch of every world
// replaces the couple and the campus, `SpawnSelf` carries the rest; the store is reset with the next login.

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';
import type { EarnedTitle } from './honorTitles';

/** `CCouple`: the EXP and the two partners; `level` is `CCouple::GetLevel( TRUE )` (computed from the EXP when the record is replaced or grows). */
export interface CoupleRecord {
  readonly exp: number;
  readonly first: number;
  readonly second: number;
  readonly level: number;
}

/** One member of the campus: `memberLevel` 1 master, 2 pupil. */
export interface CampusMemberRecord {
  readonly id: number;
  readonly level: number;
}

/** `CCampus`: the id, the master and the members (master included, as on the wire). */
export interface CampusRecord {
  readonly id: number;
  readonly master: number;
  readonly members: readonly CampusMemberRecord[];
}

/** `CTitleManager` + `g_pPlayer->m_nHonor`. */
export interface HonorState {
  /** The 150 counters of the last `HonorListAck` / `SpawnSelf` (empty before the join). */
  readonly counters: readonly number[];
  /** `m_vecEarned`: ascending id from the join, then appended as titles are earned. */
  readonly earned: readonly EarnedTitle[];
  /** `m_nHonor`: the selected title, `-1` none. */
  readonly selected: number;
}

/** `m_nCheerPoint` and `m_dwTickCheer` (as `host.now()` ms: when the next point comes). */
export interface CheerPoints {
  readonly points: number;
  readonly restUntil: number;
}

export interface RelationState {
  readonly couple: CoupleRecord | null;
  readonly campus: CampusRecord | null;
  /** `m_nCampusPoint`. */
  readonly campusPoints: number;
  readonly honor: HonorState;
  readonly cheer: CheerPoints;
}

export function initialRelationState(): RelationState {
  return {
    couple: null,
    campus: null,
    campusPoints: 0,
    honor: { counters: [], earned: [], selected: -1 },
    cheer: { points: 0, restUntil: 0 },
  };
}

export type RelationStore = StoreApi<RelationState>;

/** Defines (or returns) the `relation` store of a registry. */
export function defineRelationStore(registry: StoreRegistry): RelationStore {
  const existing = registry.get('relation');
  if (existing) return existing as unknown as RelationStore;
  return registry.defineStore<RelationState>({
    name: 'relation',
    scope: 'session',
    owner: '45',
    initial: initialRelationState,
    fixtures: {
      coupled: {
        couple: { exp: 20, first: 1, second: 2, level: 2 },
        honor: {
          counters: [],
          earned: [
            { id: 2, title: 'Casanova' },
            { id: 3, title: 'True Love' },
          ],
          selected: 2,
        },
        cheer: { points: 1, restUntil: 3_600_000 },
      },
      campus: {
        campus: {
          id: 1,
          master: 1,
          members: [
            { id: 1, level: 1 },
            { id: 2, level: 2 },
          ],
        },
        campusPoints: 12,
      },
    },
  });
}
