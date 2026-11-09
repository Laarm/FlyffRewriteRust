// `eventStore.lord` (Plan/45 §28.2, WP-CFEAT-43; ref/09b §3.16, §7.11; `src:_Common/lord.h`, `Neuz/clord.cpp`): the lord system as the client keeps it - the current lord, the election with its candidates, the
// running lord events and the cooldowns of the lord skills. `CCLord::Instance()` of the original is one global object; here it is a slice of the event store. Every message of the lord protocol is
// applied whether or not a window is open (PL-124 "list parsing without a window"): a window that opens later reads the slice.

/** `NULL_ID` of the wire (`Lord.idLord`, `ElectionEndVote.idLord`: no lord). */
export const LORD_NULL_ID = 0xffff_ffff;

/** `IElection::ELECTION_STATE`. */
export const ELECTION_STATE = { Ready: 0, Candidacy: 1, Vote: 2, Expired: 3 } as const;
export type ElectionState = (typeof ELECTION_STATE)[keyof typeof ELECTION_STATE];

/** `IElection::nMaxCandidates`: the vote starts with the first ten. */
export const MAX_CANDIDATES = 10;
/** `CCandidate::nMaxPledgeLen`, `CCandidate::nMinDeposit` (the lowest total deposit), `LI_SUMMON` and `LI_TELEPORT` (the two skills that ask for a name): one definition, the window ports'. */
export {
  LI_SUMMON,
  LI_TELEPORT,
  MAX_PLEDGE_BYTES as MAX_PLEDGE,
  MIN_DEPOSIT,
} from '../../../ui/windows/lord/ports';
/** The lord skills (`LI_CHEERING` .. `LI_PARTY`, `lordskill.txt` ids 0..7). */
export const LORD_SKILL_COUNT = 8;

/** `CCandidate`. */
export interface LordCandidate {
  /** `m_idPlayer`. */
  readonly id: number;
  /** `m_iDeposit` in penya. */
  readonly deposit: number;
  /** `m_nVote`. */
  readonly votes: number;
  /** `m_tCreate`: unix seconds of the first deposit. */
  readonly createdAt: number;
  /** `m_szPledge`. */
  readonly pledge: string;
}

/** `IElection::sProperty` (the client keeps what the wire carries; only `days` is read by the reducers). */
export interface ElectionProperty {
  readonly dayOfWeek: number;
  readonly hour: number;
  /** `tCandidacy` in seconds. */
  readonly candidacyS: number;
  /** `tVote` in seconds. */
  readonly voteS: number;
  readonly requirementFactor: number;
  /** `nDays`: the days between two elections. */
  readonly days: number;
  readonly rates: readonly number[];
  readonly items: readonly (readonly number[])[];
}

/** `IElection`. */
export interface Election {
  /** `m_idElection`. */
  readonly id: number;
  readonly state: ElectionState;
  /** `m_tBegin`: unix seconds, 0 = not scheduled. */
  readonly beginAt: number;
  /** `m_nRequirement`: the votes needed. */
  readonly requirement: number;
  readonly property: ElectionProperty;
  readonly candidates: readonly LordCandidate[];
}

/** `CLEComponent`: one running lord event. */
export interface LordEventComponent {
  /** `idPlayer`: the creator. */
  readonly creator: number;
  /** `nTick`: minutes left. */
  readonly tick: number;
  /** `fEFactor`. */
  readonly expFactor: number;
  /** `fIFactor`. */
  readonly itemFactor: number;
}

export interface LordSlice {
  /** `CLord::m_idPlayer`; `null` = none (`NULL_ID`). */
  readonly lord: number | null;
  readonly election: Election;
  readonly events: readonly LordEventComponent[];
  /** `CLordSkillComponent::m_nTick` by skill id: the minutes left of the cooldown (0 = ready). */
  readonly skillTicks: readonly number[];
  /** `ctx.now()` in ms of the last change of `skillTicks` (the cooldown radar counts the minutes from it). */
  readonly skillTicksAt: number;
}

export function initialProperty(): ElectionProperty {
  return {
    dayOfWeek: 0,
    hour: 0,
    candidacyS: 0,
    voteS: 0,
    requirementFactor: 0,
    days: 0,
    rates: [],
    items: [],
  };
}

export function initialElection(): Election {
  return {
    id: 0,
    state: ELECTION_STATE.Ready,
    beginAt: 0,
    requirement: 0,
    property: initialProperty(),
    candidates: [],
  };
}

export function initialLord(): LordSlice {
  return {
    lord: null,
    election: initialElection(),
    events: [],
    skillTicks: Array.from({ length: LORD_SKILL_COUNT }, () => 0),
    skillTicksAt: 0,
  };
}
