// `pvpStore` (Plan/45 §2.3, §25.1, §7.10; WP-CFEAT-36): the own duel fields (the mirror of `m_nDuel`, `m_nDuelState`, `m_idDuelOther`
// and `m_idDuelParty` of the player), the duel request that waits for an answer, the 3-2-1 countdown of `DuelCount` and the list of the
// DPS meter. Scope `world`: everything is void in the next world (the duel fields live on the mover of the old session, the meter list is
// per fight).

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';

/** `NULL_ID`: no object (`m_idDuelOther` of a mover without a solo duel). */
export const NULL_ID = 0xffffffff;

/** One row of the DPS list (`__DPSPLAYER`): the damage is a C `int` of the message. */
export interface DpsEntry {
  readonly idPlayer: number;
  readonly name: string;
  readonly damage: number;
}

/** A duel challenge that waits for the answer of the confirm window (`CWndDuelConfirm`). */
export interface DuelRequestInfo {
  /** The requester's character id (`uidSrc`). */
  readonly src: number;
  /** The requester's object id. */
  readonly objId: number;
  readonly name: string;
  /** `m_bDuelParty`: a party duel. */
  readonly party: boolean;
}

/** The `DuelCount` the overlay shows (`m_nPVPCount`, `m_dwPVPTime`). */
export interface DuelCountdown {
  /** 3, 2, 1, then 0 for "fight". */
  readonly n: number;
  /** `ctx.now()` of the message. */
  readonly at: number;
  /** Increases with every message (the overlay restarts its animation). */
  readonly seq: number;
}

export interface PvpState {
  /** `m_nDuel`: 0 none, 1 solo, 2 party. */
  readonly duel: number;
  /** `m_nDuelState`: 104..100 countdown, 1 fighting, 300 out, 0 none. */
  readonly duelState: number;
  /** `m_idDuelOther` (an object id, `NULL_ID` for none). */
  readonly duelOther: number;
  /** `m_idDuelParty`: the party id of the opposing party (0 none). */
  readonly duelParty: number;
  /** The name of the solo opponent (from the start message), for the HUD and the probe. */
  readonly opponent: string;
  /** The name of the opposing party of a party duel. */
  readonly partyName: string;
  readonly request: DuelRequestInfo | null;
  readonly count: DuelCountdown | null;
  /** The DPS meter: `show` is `m_bShowDPS`, the entries are sorted by damage, descending. */
  readonly dps: { readonly show: boolean; readonly entries: readonly DpsEntry[] };
}

export function initialPvpState(): PvpState {
  return {
    duel: 0,
    duelState: 0,
    duelOther: NULL_ID,
    duelParty: 0,
    opponent: '',
    partyName: '',
    request: null,
    count: null,
    dps: { show: false, entries: [] },
  };
}

export type PvpStore = StoreApi<PvpState>;

/** The store of a registry (created on first use). */
export function definePvpStore(registry: StoreRegistry): PvpStore {
  const existing = registry.get('pvp');
  if (existing) return existing as unknown as PvpStore;
  return registry.defineStore<PvpState>({
    name: 'pvp',
    scope: 'world',
    owner: '45',
    initial: initialPvpState,
    fixtures: {
      dueling: { duel: 1, duelState: 1, duelOther: 7, opponent: 'Aibatt' },
      countdown: { duel: 1, duelState: 102, duelOther: 7, opponent: 'Aibatt' },
    },
  });
}
