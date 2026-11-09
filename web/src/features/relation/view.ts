// The read models of the couple, campus and honor windows (Plan/45 §24.2, §24.3, §2.8; WP-CFEAT-40): the relation mirror and the player-data cache become the views of 42's
// `CoupleInfoView`, `CoupleSkillView`, `HonorView` and `CampusView`. Pure: the module gathers the facts, so the views are tested without a client.

import { JTYPE_HERO, JTYPE_MASTER } from '../../gen/defs/job';
import { masterMarkSprite } from '../../ui/windows/messenger/icons';
import {
  type CampusView,
  EMPTY_CAMPUS_VIEW,
  FRS,
  type MessengerRow,
} from '../../ui/windows/messenger/ports';
import {
  EMPTY_COUPLE_INFO,
  type CoupleInfoView,
  type CoupleSkillView,
  type HonorView,
} from '../../ui/windows/couple/ports';
import { coupleRate, gaugeRate, skillRowAt, type SkillLevelRow } from './coupleLevel';
import { type CampusRecord, type CoupleRecord, type RelationState } from './store';
import { isCampusMaster, isCampusPupil, partnerOf } from './reducers';

/** The cached data of one character (`PlayerDataEntry`, trimmed to what the views read). */
export interface CachedPlayer {
  readonly name: string;
  readonly level: number;
  readonly job: number;
  readonly online: boolean;
  /** The data version; 0 = never answered (a placeholder). */
  readonly ver: number;
}

export interface ViewFacts {
  /** The character id of the own character. */
  readonly self: number;
  readonly cached: (idPlayer: number) => CachedPlayer | undefined;
  /** `prj.m_aJob[ job ]`: the job type (`JTYPE_*`) and name; the data may not know the job. */
  readonly job: (job: number) => { readonly type: number; readonly name: string };
}

/** A cache entry the views may show: it was answered (`ver > 0`) and carries a name. */
const known = (c: CachedPlayer | undefined): c is CachedPlayer =>
  c !== undefined && c.ver > 0 && c.name !== '';

/**
 * The master / hero mark beside the level of a character (`CWndCoupleTabInfo::OnDraw`): a master gets the mark of its level band (27..32), a hero 33, anyone else none. The sprites
 * are those of `icon/icon_playerdata.inc`.
 */
export function markSprite(jobType: number, level: number): number | null {
  if (jobType === JTYPE_MASTER) return masterMarkSprite(level);
  if (jobType === JTYPE_HERO) return 33;
  return null;
}

/** The couple page: the partner as the cache knows it, the couple level and the gauge fill. */
export function coupleInfoView(
  couple: CoupleRecord | null,
  levelExp: readonly number[],
  facts: ViewFacts,
): CoupleInfoView {
  if (!couple) return EMPTY_COUPLE_INFO;
  const partner = facts.cached(partnerOf(couple, facts.self));
  const base = {
    coupled: true,
    coupleLevel: String(couple.level),
    rate: gaugeRate(coupleRate(levelExp, couple.level, couple.exp)),
  };
  if (!known(partner)) {
    return { ...base, name: '', job: '', level: '', mark: null, canBreak: false };
  }
  const job = facts.job(partner.job);
  return {
    ...base,
    name: partner.name,
    job: job.name,
    level: String(partner.level),
    mark: markSprite(job.type, partner.level),
    canBreak: true,
  };
}

/** What the skill page reads of the data. */
export interface SkillFacts {
  readonly kinds: readonly number[];
  readonly rows: readonly SkillLevelRow[];
  /** The item of a skill kind: its name and `item/` icon path. */
  readonly item: (
    id: number,
  ) => { readonly name: string; readonly icon: string | null } | undefined;
  /** The text panel `coupleskillinfo` (`''` while the help bundle is not loaded). */
  readonly info: string;
}

/** The skill page: three cells, an icon lit when the couple level reaches a level of that kind (none while the player is single). */
export function coupleSkillView(couple: CoupleRecord | null, facts: SkillFacts): CoupleSkillView {
  const levels = couple ? skillRowAt(facts.rows, couple.level) : [];
  // `if( vSkills.size() == vSkillKinds.size() )`
  const usable = couple !== null && levels.length === facts.kinds.length;
  return {
    cells: facts.kinds.map((kind, i) => {
      const item = facts.item(kind);
      return {
        name: item?.name ?? '',
        icon: item?.icon ?? null,
        lit: usable && (levels[i] ?? 0) > 0,
      };
    }),
    info: facts.info,
  };
}

/** The honor page: the earned list as the store has it; the identity changes with every list the server sent (the page refreshes on each `HonorListAck`). */
export function honorView(state: RelationState): HonorView {
  return { titles: state.honor.earned };
}

/** The row of a campus member from the cache (`CWndCampus::UpdatePlayerList`): online when `login > 0`, the channel is the single channel of this build. */
function campusRow(id: number, facts: ViewFacts): MessengerRow | null {
  const c = facts.cached(id);
  if (!known(c)) return null;
  const info = facts.job(c.job);
  return {
    id,
    name: c.name,
    level: c.level,
    job: c.job,
    jobType: info.type,
    jobName: info.name,
    state: c.online ? FRS.ONLINE : FRS.OFFLINE,
    channel: c.online ? 1 : 0,
    blocked: false,
  };
}

/**
 * The campus page: the master of a pupil, or the pupils of a master. A member the cache does not know yet has no row (the original skipped a missing `PlayerData`; a placeholder
 * without a name would draw an empty line).
 */
export function campusViewOf(campus: CampusRecord | null, facts: ViewFacts): CampusView {
  if (!campus) return EMPTY_CAMPUS_VIEW;
  if (isCampusMaster(campus, facts.self)) {
    const disciples = campus.members
      .filter((m) => m.level === 2)
      .map((m) => campusRow(m.id, facts))
      .filter((r): r is MessengerRow => r !== null);
    return { role: 'master', master: null, disciples };
  }
  if (isCampusPupil(campus, facts.self)) {
    return { role: 'pupil', master: campusRow(campus.master, facts), disciples: [] };
  }
  return EMPTY_CAMPUS_VIEW;
}
