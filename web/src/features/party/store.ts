// `partyStore` (Plan/45 §2.3, §22; WP-CFEAT-34, -35): the mirror of `g_Party` (`CParty`, `_Common/party.h`) with the pieces of the client that
// hang on it: the party record (members in slot order, slot 0 is the leader, the offline flags, level / EXP / points, the share modes, the
// four mode flags), the allow-join flag of the finder, the last party name of the character (`m_szPartyName`), the positions of the members out of
// sight (`PartyMapInfo`), the navigator marks of the party (`SetNaviPointNtf`) and the list of the party finder. Scope `world`: a new
// character (or a return to the character list) starts without a party; a world change keeps it (the server re-sends the record in the join
// batch anyway).
//
// The chat input asks `ctx.stores.registry.get('party')` for `members.length` (`/p` needs two members).

import type { StoreApi } from 'zustand/vanilla';
import type { ClientContext } from '../../app/context';
import type { StoreRegistry } from '../../stores/registry';

/** `PARTY_LINKATTACK_MODE` .. `PARTY_PARSKILL_MODE` (`MAX_PARTYMODE` = 5). */
export const PARTY_MODE = {
  LINKATTACK: 0,
  FORTUNECIRCLE: 1,
  STRETCHING: 2,
  GIFTBOX: 3,
  PARSKILL: 4,
} as const;
export const MAX_PARTYMODE = 5;

/** The party skill ids of `defineSkill.h` (`ST_CALL` .. `ST_GIFTBOX`). */
export const ST = {
  CALL: 1,
  BLITZ: 2,
  RETREAT: 3,
  SPHERECIRCLE: 4,
  LINKATTACK: 5,
  FORTUNECIRCLE: 6,
  STRETCHING: 7,
  GIFTBOX: 8,
} as const;

/** The party skill that runs a party mode (`ST_LINKATTACK` .. `ST_GIFTBOX` -> mode 0..3); `-1` for the instant skills. */
export function modeOfSkill(skill: number): number {
  return skill >= ST.LINKATTACK && skill <= ST.GIFTBOX ? skill - ST.LINKATTACK : -1;
}

/** The party skill of a mode 0..3 (`-1` for PARSKILL, which has no skill). */
export function skillOfMode(mode: number): number {
  return mode >= PARTY_MODE.LINKATTACK && mode <= PARTY_MODE.GIFTBOX ? mode + ST.LINKATTACK : -1;
}

/** One slot of the party (`PartyMember`: the character id and `m_bRemove`, the offline flag). */
export interface PartyMember {
  readonly id: number;
  readonly offline: boolean;
}

/** One entry of the finder list (`PARTYFINDER_LIST`). */
export interface FinderEntry {
  readonly partyId: number;
  readonly name: string;
  readonly members: number;
  readonly level: number;
  readonly point: number;
  readonly leaderId: number;
  readonly leaderName: string;
  readonly memberList: string;
  readonly linkAttack: boolean;
  readonly fortuneCircle: boolean;
  readonly giftBox: boolean;
  readonly skillFull: boolean;
  readonly expUp: boolean;
  readonly skillUp: boolean;
}

/** A minimap marker of a party member (`NaviPoint`: the position, the name, the sender's object id, and when it was set). */
export interface NaviMark {
  readonly objId: number;
  readonly name: string;
  readonly pos: readonly [number, number, number];
  /** `ctx.now()` of the message. */
  readonly at: number;
}

export interface PartyState {
  /** `m_uPartyId`, 0 = no party. */
  readonly id: number;
  /** `m_nKindTroup`: 0 normal, 1 advanced. */
  readonly kind: number;
  /** `m_sParty` (advanced parties). */
  readonly name: string;
  /** `m_nLevel` (1 without a party). */
  readonly level: number;
  readonly exp: number;
  readonly point: number;
  /** `m_nTroupsShareExp`: 0 by level, 1 by contribution. */
  readonly shareExp: number;
  /** `m_nTroupeShareItem`: 0 finder, 1 round robin, 2 leader, 3 random. */
  readonly shareItem: number;
  /** `m_idDuelParty`. */
  readonly duelParty: number;
  /** `m_nModeTime[5]` as the client keeps it after a `SetPartyMode`: 1 running, 0 off. */
  readonly modes: readonly number[];
  /** `ctx.now()` at which a running mode 0..3 ends (the wire carries the remaining time); 0 = unknown. */
  readonly modeEnds: readonly number[];
  /** Slot 0 is the leader. */
  readonly members: readonly PartyMember[];
  /** The leader's allow-join flag (`m_bPartyJoin`, default on). */
  readonly allowJoin: boolean;
  /** `m_szPartyName`: the last party name this character used (`AddPartyName`, a name the party got). */
  readonly ownName: string;
  /** `PartyMapInfo`: the last known position of a member out of sight, by character id. */
  readonly mapPos: Readonly<Record<number, readonly [number, number, number]>>;
  /** The party marks of the navigator, newest last (at most 10). */
  readonly marks: readonly NaviMark[];
  /** The finder list of the last `PartyFinderRefresh`. */
  readonly finder: readonly FinderEntry[];
  /** Increases with every `PartyFinderRefresh` (windows redraw on it). */
  readonly finderSeq: number;
}

export function initialPartyState(): PartyState {
  return {
    id: 0,
    kind: 0,
    name: '',
    level: 1,
    exp: 0,
    point: 0,
    shareExp: 0,
    shareItem: 0,
    duelParty: 0,
    modes: [0, 0, 0, 0, 0],
    modeEnds: [0, 0, 0, 0, 0],
    members: [],
    allowJoin: true,
    ownName: '',
    mapPos: {},
    marks: [],
    finder: [],
    finderSeq: 0,
  };
}

/** The party record alone: `InitParty()` (the finder, the marks and the own name are not part of it). */
export function emptyRecord(): Pick<
  PartyState,
  | 'id'
  | 'kind'
  | 'name'
  | 'level'
  | 'exp'
  | 'point'
  | 'shareExp'
  | 'shareItem'
  | 'duelParty'
  | 'modes'
  | 'modeEnds'
  | 'members'
  | 'mapPos'
> {
  const s = initialPartyState();
  return {
    id: s.id,
    kind: s.kind,
    name: s.name,
    level: s.level,
    exp: s.exp,
    point: s.point,
    shareExp: s.shareExp,
    shareItem: s.shareItem,
    duelParty: s.duelParty,
    modes: s.modes,
    modeEnds: s.modeEnds,
    members: s.members,
    mapPos: s.mapPos,
  };
}

export type PartyStore = StoreApi<PartyState>;

/** The store of a registry (created on first use). */
export function definePartyStore(registry: StoreRegistry): PartyStore {
  const existing = registry.get('party');
  if (existing) return existing as unknown as PartyStore;
  return registry.defineStore<PartyState>({
    name: 'party',
    scope: 'world',
    owner: '45',
    initial: initialPartyState,
    fixtures: {
      pair: {
        id: 7,
        level: 3,
        exp: 40,
        members: [
          { id: 1, offline: false },
          { id: 2, offline: false },
        ],
      },
      advanced: {
        id: 7,
        kind: 1,
        name: 'Fellowship',
        level: 12,
        exp: 100,
        point: 50,
        shareItem: 1,
        members: [
          { id: 1, offline: false },
          { id: 2, offline: false },
          { id: 3, offline: true },
        ],
      },
    },
  });
}

/** `IsLeader(id)`: the character is slot 0. */
export function isLeader(s: Pick<PartyState, 'members'>, id: number): boolean {
  return id !== 0 && s.members[0]?.id === id;
}

/** `FindMember(id)`: the slot of a character, -1 when it is not a member. */
export function findMember(s: Pick<PartyState, 'members'>, id: number): number {
  return s.members.findIndex((m) => m.id === id);
}

/** `g_Party.IsLeader( g_pPlayer->m_idPlayer )` of a client context; `false` before the party store exists (the item router asks). */
export function selfIsPartyLeader(ctx: ClientContext): boolean {
  const party = ctx.stores.registry.get('party')?.getState() as
    Pick<PartyState, 'members'> | undefined;
  return party ? isLeader(party, ctx.stores.selfStore.getState().idPlayer) : false;
}
