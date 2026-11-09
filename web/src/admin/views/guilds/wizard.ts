// The state of the Create guild wizard (Plan/25 section 6.2) as pure functions: the form, the byte counter of the notice, the free
// places of each rank for the chosen level, which step is complete, and the request body (equal to the `GuildSpec` DTO). The server
// validates everything again; this only keeps the operator from sending what it will refuse.
import type { GuildMeta, GuildRank, GuildSpec } from '../../gen/dto';

export interface PickedCharacter {
  readonly id: number;
  readonly name: string;
  readonly online: boolean;
}

export interface WizardMember extends PickedCharacter {
  readonly rank: GuildRank;
}

export interface WizardState {
  readonly name: string;
  readonly master: PickedCharacter | null;
  readonly members: readonly WizardMember[];
  readonly overrideCooldown: boolean;
  /** 0 = none, 1..27. */
  readonly logo: number;
  readonly level: number;
  readonly gold: number;
  readonly pxp: number;
  readonly notice: string;
  readonly giveCloak: boolean;
}

export const INITIAL: WizardState = {
  name: '',
  master: null,
  members: [],
  overrideCooldown: false,
  logo: 0,
  level: 1,
  gold: 0,
  pxp: 0,
  notice: '',
  giveCloak: false,
};

/** The bytes of a notice (the server counts UTF-8 bytes, at most `notice_max_bytes`). */
export function noticeBytes(text: string): number {
  return new TextEncoder().encode(text).length;
}

const clampInt = (v: number, lo: number, hi: number): number =>
  Math.min(hi, Math.max(lo, Number.isFinite(v) ? Math.trunc(v) : lo));

export function clampLevel(v: number): number {
  return clampInt(v, 1, 50);
}

export function clampGold(v: number, meta: Pick<GuildMeta, 'gold_max'>): number {
  return clampInt(v, 0, meta.gold_max);
}

export function clampPxp(v: number): number {
  return clampInt(v, 0, 0xffff_ffff);
}

/** The member limit of a level (`levels[level - 1]`, the last known when the table is shorter). */
export function maxMembers(level: number, meta: Pick<GuildMeta, 'levels'>): number {
  const row = meta.levels.find((l) => l.level === level) ?? meta.levels[meta.levels.length - 1];
  return row?.max_members ?? 30;
}

export interface RankPlaces {
  readonly rank: GuildRank;
  readonly cap: number;
  readonly used: number;
  readonly free: number;
}

const RANKS: readonly GuildRank[] = ['master', 'kingpin', 'captain', 'supporter', 'member'];

/** The cap of a rank at a level: 1 / 5 / 10 / 20, and the member limit for the Rookies. */
export function rankCap(
  rank: GuildRank,
  level: number,
  meta: Pick<GuildMeta, 'levels' | 'rank_caps'>,
): number {
  const i = RANKS.indexOf(rank);
  return i < 4 ? (meta.rank_caps[i] ?? 0) : maxMembers(level, meta);
}

/** Per rank: the cap, the places used by the master and the chosen members, and what is free (also bounded by the member limit). */
export function rankPlaces(
  state: Pick<WizardState, 'level' | 'master' | 'members'>,
  meta: Pick<GuildMeta, 'levels' | 'rank_caps'>,
): RankPlaces[] {
  const total = (state.master ? 1 : 0) + state.members.length;
  const roomLeft = Math.max(0, maxMembers(state.level, meta) - total);
  return RANKS.map((rank) => {
    const used =
      (rank === 'master' && state.master ? 1 : 0) +
      state.members.filter((m) => m.rank === rank).length;
    const cap = rankCap(rank, state.level, meta);
    return { rank, cap, used, free: Math.max(0, Math.min(cap - used, roomLeft)) };
  });
}

/** `true` when no rank is over its cap and the roster fits the level. */
export function rosterFits(
  state: Pick<WizardState, 'level' | 'master' | 'members'>,
  meta: Pick<GuildMeta, 'levels' | 'rank_caps'>,
): boolean {
  const total = (state.master ? 1 : 0) + state.members.length;
  return (
    total <= maxMembers(state.level, meta) && rankPlaces(state, meta).every((p) => p.used <= p.cap)
  );
}

export type StepId = 'name' | 'members' | 'logo' | 'settings';

export interface NameState {
  /** The answer of `validate-name` for the current text: `ok` true when free. */
  readonly ok: boolean | null;
}

/** Which steps are complete (the logo step always is: "None" is a choice). */
export function stepsDone(
  state: WizardState,
  meta: GuildMeta,
  name: NameState,
): Record<StepId, boolean> {
  return {
    name: name.ok === true,
    members: state.master !== null && rosterFits(state, meta),
    logo: true,
    settings: noticeBytes(state.notice) <= meta.notice_max_bytes,
  };
}

/** Why the guild cannot be created yet, `null` when it can. */
export function blockedReason(
  state: WizardState,
  meta: GuildMeta,
  name: NameState,
): 'name' | 'master' | 'members' | 'notice' | null {
  const done = stepsDone(state, meta, name);
  if (!done.name) return 'name';
  if (state.master === null) return 'master';
  if (!done.members) return 'members';
  if (!done.settings) return 'notice';
  return null;
}

/** The request body: exactly the `GuildSpec` of the DTO. `give_cloak` is only meaningful with a logo. */
export function toSpec(state: WizardState): GuildSpec {
  return {
    name: state.name.trim(),
    master: state.master?.id ?? 0,
    members: state.members.map((m): [number, GuildRank] => [m.id, m.rank]),
    level: clampLevel(state.level),
    gold: Math.max(0, Math.trunc(state.gold)),
    pxp: Math.max(0, Math.trunc(state.pxp)),
    logo: state.logo,
    notice: state.notice,
    give_cloak: state.giveCloak && state.logo !== 0,
    override_cooldown: state.overrideCooldown,
  };
}

/** The step a server error belongs to (`details.field` of an `invalid` answer, or a `stale_target` about the name or a busy character). */
export function stepOfField(field: string | undefined): StepId | null {
  switch (field) {
    case 'name':
      return 'name';
    case 'master':
    case 'members':
    case 'character':
      return 'members';
    case 'logo':
      return 'logo';
    case 'level':
    case 'gold':
    case 'notice':
      return 'settings';
    case undefined:
    default:
      return null;
  }
}
