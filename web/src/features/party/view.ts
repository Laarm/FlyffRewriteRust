// The read models of the party windows (Plan/45 §22.2, §7.8, §22.3; WP-CFEAT-34, -35): the party mirror, the player-data cache and the movers
// in view become the `PartyView` and `FinderView` of 42. Pure: `buildPartyView` takes plain facts (the module gathers them), so the
// numbers (the EXP percent, the level marks, the status colours, the skill cells) are tested without a client.

import type { ClientItem } from '../../data/clientData';
import {
  TID_GAME_LEADER,
  TID_GAME_MEMBER,
  TID_GAME_PARTY1,
  TID_GAME_PARTY2,
  TID_GAME_QUICK_MARK_HERO,
  TID_GAME_QUICK_MARK_MASTER,
  TID_GAME_TOOLTIP_MARK_HERO,
  TID_GAME_TOOLTIP_MARK_MASTER,
} from '../../gen/defs/text';
import type {
  FinderRow,
  FinderView,
  MemberStatus,
  PartyMemberRow,
  PartySkillRow,
  PartyView,
  ViewSource,
} from '../../ui/windows/party/ports';
import { formatTemplate } from '../common/text';
import { isHeroJob, isMasterJob } from '../hud/targetFrame/model';
import { MAX_PARTYLEVEL } from './reducers';
import { skillCellState } from './skills';
import { type FinderEntry, isLeader, type PartyState, ST } from './store';

/** What the view needs to know about one member who is in view (`prj.GetUserByID`). */
export interface MoverFacts {
  readonly hp: number;
  /** `0` when the maximum is not known. */
  readonly maxHp: number;
  readonly name: string;
  readonly level: number;
  readonly job: number;
}

/** The cached data of one character (`PlayerDataEntry`, trimmed to what the rows read). */
export interface CachedFacts {
  readonly name: string;
  readonly level: number;
  readonly job: number;
  readonly answered: boolean;
}

export interface ViewFacts {
  readonly party: PartyState;
  /** The own character id. */
  readonly selfId: number;
  /** `ctx.now()` ms. */
  readonly now: number;
  readonly text: (tid: number) => string;
  /** The mover of a member in view, `null` when it is not. */
  readonly mover: (idPlayer: number) => MoverFacts | null;
  readonly cached: (idPlayer: number) => CachedFacts | undefined;
  readonly jobName: (job: number) => string;
  /** `prj.m_aExpParty` rows `[exp, point]`. */
  readonly expParty: readonly (readonly [number, number])[];
  /** `prj.GetPartySkill(id)`. */
  readonly partySkill: (id: number) => ClientItem | undefined;
  readonly dueling: boolean;
  readonly findEnabled: boolean;
}

/** `"%.2f %%"` of the experience of the party: the denominator of level 10 and up is `((50 + level) * level / 13) * 10` (C `int` arithmetic). */
export function partyExpText(
  party: Pick<PartyState, 'level' | 'exp'>,
  expParty: readonly (readonly [number, number])[],
): string {
  const denom =
    party.level >= MAX_PARTYLEVEL
      ? Math.trunc(((50 + party.level) * party.level) / 13) * 10
      : (expParty[party.level]?.[0] ?? 0);
  // `(float)exp * 100 / (float)denom`, printed with two decimals
  const pct = denom === 0 ? 0 : Math.fround(Math.fround(party.exp * 100) / Math.fround(denom));
  return `${pct.toFixed(2)} %`;
}

/** `"mm:ss"` of the time a running party mode has left. */
export function modeRemainText(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const m = Math.trunc(total / 60);
  const s = total % 60;
  return `${String(m)}:${String(s).padStart(2, '0')}`;
}

/** The marks after the level: `-M` for a master, `-H` for a hero. */
function levelText(level: number, job: number, text: (tid: number) => string): string {
  if (isMasterJob(job)) return `${String(level)}${text(TID_GAME_TOOLTIP_MARK_MASTER)}`;
  if (isHeroJob(job)) return `${String(level)}${text(TID_GAME_TOOLTIP_MARK_HERO)}`;
  return String(level);
}

/** The line of the quick window: `"<level>. <name>"`, the master / hero formats of the texts for those jobs. */
function quickLine(
  level: number,
  job: number,
  name: string,
  text: (tid: number) => string,
): string {
  if (isMasterJob(job)) return formatTemplate(text(TID_GAME_QUICK_MARK_MASTER), [level, name]);
  if (isHeroJob(job)) return formatTemplate(text(TID_GAME_QUICK_MARK_HERO), [level, name]);
  return `${String(level)}. ${name}`;
}

function memberRows(f: ViewFacts): PartyMemberRow[] {
  return f.party.members.map((m, i) => {
    const mover = f.mover(m.id);
    const cached = f.cached(m.id);
    const inView = mover !== null;
    // the cache is the source of level and job (it follows `UpdatePlayerData`); a character it has not answered for yet shows the spawn
    const answered = cached?.answered === true;
    const level = answered ? cached.level : (mover?.level ?? cached?.level ?? 1);
    const job = answered ? cached.job : (mover?.job ?? cached?.job ?? 0);
    const name =
      cached?.name !== undefined && cached.name !== '' ? cached.name : (mover?.name ?? '');
    let status: MemberStatus;
    let hp = 0;
    if (mover) {
      status = 'ok';
      if (mover.hp <= 0) status = 'dead';
      else if (mover.maxHp > 0 && mover.hp / mover.maxHp < 0.1) status = 'low';
      hp =
        mover.maxHp > 0 ? Math.max(0, Math.min(1, mover.hp / mover.maxHp)) : mover.hp > 0 ? 1 : 0;
    } else {
      status = m.offline ? 'offline' : 'away';
    }
    return {
      id: m.id,
      leader: i === 0,
      offline: m.offline,
      inView,
      status,
      levelText: levelText(level, job, f.text),
      classText: f.text(i === 0 ? TID_GAME_LEADER : TID_GAME_MEMBER),
      jobName: f.jobName(job),
      name,
      quickText: quickLine(level, job, name, f.text),
      hp,
    };
  });
}

/** The nine cells of the skill tab (the data has eight). */
function skillRows(f: ViewFacts): PartySkillRow[] {
  const rows: PartySkillRow[] = [];
  for (let id: number = ST.CALL; id <= ST.GIFTBOX; id += 1) {
    const prop = f.partySkill(id);
    if (!prop) continue;
    const mode = id >= ST.LINKATTACK ? id - ST.LINKATTACK : -1;
    const end = mode >= 0 ? (f.party.modeEnds[mode] ?? 0) : 0;
    const left = end - f.now;
    rows.push({
      id,
      name: prop.name,
      icon: prop.icon === '' ? null : `icon/${prop.icon}`,
      state: skillCellState(f.party.kind, f.party.level, f.party.point, prop.req_dis_lv, prop.exp),
      remain: mode >= 0 && f.party.modes[mode] === 1 && left > 0 ? modeRemainText(left) : '',
      dragAllowed: f.party.level - prop.req_dis_lv >= 0,
    });
  }
  return rows;
}

/** The view of the party windows. */
export function buildPartyView(f: ViewFacts): PartyView {
  const p = f.party;
  const leader = isLeader(p, f.selfId);
  return {
    members: memberRows(f),
    kind: p.kind,
    nameText:
      p.kind === 0 ? f.text(TID_GAME_PARTY1) : p.name === '' ? f.text(TID_GAME_PARTY2) : p.name,
    level: p.level,
    expText: partyExpText(p, f.expParty),
    point: p.point,
    shareExp: p.shareExp,
    shareItem: p.shareItem,
    allowJoin: p.allowJoin,
    leader,
    canChange: p.level >= MAX_PARTYLEVEL && leader,
    dueling: f.dueling,
    findEnabled: f.findEnabled,
    skills: skillRows(f),
  };
}

// ---- the finder -----------------------------------------------------------------------------------------------------------------

/** A wire entry as the finder list shows it. */
export function finderRow(e: FinderEntry): FinderRow {
  return {
    partyId: e.partyId,
    name: e.name,
    leaderName: e.leaderName,
    leaderId: e.leaderId,
    sizeText: `${String(e.members)}/8`,
    level: e.level,
    point: e.point,
    memberList: e.memberList,
    flags: [e.skillFull, e.expUp, e.skillUp, e.linkAttack, e.fortuneCircle, e.giftBox],
  };
}

export function buildFinderView(
  entries: readonly FinderEntry[],
  flagIcons: readonly (string | null)[],
): FinderView {
  return { rows: entries.map(finderRow), flagIcons };
}

// ---- a live view -----------------------------------------------------------------------------------------------------------------

/** Structural equality of two plain values (primitives, arrays and objects of them). */
export function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((v, i) => deepEqual(v, b[i]));
  }
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return ka.every((k) =>
    deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]),
  );
}

/** Anything with the zustand `subscribe` shape. */
export interface Observable {
  subscribe(listener: () => void): () => void;
}

/**
 * A view whose `get()` always recomputes (the HP of a member in view is not in any store: the windows poll at 10 Hz) and keeps the
 * previous object while the new one is structurally equal; `subscribe` fires when one of the sources changed AND the value did.
 */
export function liveView<T>(compute: () => T, sources: readonly Observable[]): ViewSource<T> {
  // computed on the first read: the facts (the data bundles, the entity table) may not exist yet when the module registers
  let value: T | undefined;
  const listeners = new Set<() => void>();
  let detach: (() => void)[] = [];
  const refresh = (): T => {
    const next = compute();
    if (value !== undefined && deepEqual(value, next)) return value;
    value = next;
    return next;
  };
  const onSource = (): void => {
    const before = value;
    if (refresh() !== before) for (const fn of [...listeners]) fn();
  };
  return {
    get: refresh,
    subscribe(fn) {
      listeners.add(fn);
      if (listeners.size === 1) {
        refresh();
        detach = sources.map((s) => s.subscribe(onSource));
      }
      return () => {
        listeners.delete(fn);
        if (listeners.size === 0) {
          for (const d of detach) d();
          detach = [];
        }
      };
    },
  };
}
