// The party-skill branch of the taskbar shortcut port `skills.useShortcut(type 2, id)` (Plan/45 §19.4; `CWndMgr::ObjectExecutor`,
// `src:_Interface/WndManager.cpp:2862-2950`; WP-CFEAT-35). Pure: the module gathers the facts, this function decides.
//
//   * the party has fewer than two members or is a normal party, or the skill has no row: nothing happens;
//   * not the leader: the box `TID_DIAG_0047`;
//   * party level below the skill's `req_dis_lv`: the box `TID_DIAG_0019`;
//   * fewer party points than the skill's cost (`prop.exp`): the box `TID_DIAG_0049`;
//   * the skill's party buff is still running and the leader has none of the item buffs PARTYSKILLUP01 / 02 / 01_01 (they allow the time
//     to be added): the chat line `TID_GAME_NOT_TIME(skill name)` - not for CALL, BLITZ and RETREAT, which have no buff (they never reach
//     this branch in practice);
//   * otherwise `PartySkillUse{skill}`.

import {
  TID_DIAG_0019,
  TID_DIAG_0047,
  TID_DIAG_0049,
  TID_GAME_NOT_TIME,
} from '../../gen/defs/text';
import { ST } from './store';

/** The facts of the decision. */
export interface PartySkillFacts {
  /** `g_Party.m_nSizeofMember`. */
  readonly size: number;
  /** `g_Party.m_nKindTroup`. */
  readonly kind: number;
  readonly leader: boolean;
  readonly level: number;
  readonly point: number;
  /** The `propTroupeSkill` row: required party level, point cost, name; `undefined` for an unknown skill. */
  readonly prop:
    { readonly reqLevel: number; readonly cost: number; readonly name: string } | undefined;
  /** The party buff icon of this skill exists (`m_buffs` holds the id: the mode is running). */
  readonly running: boolean;
  /** The leader has `PARTYSKILLUP01`, `PARTYSKILLUP02` or `PARTYSKILLUP01_01` (`BUFF_ITEM2`). */
  readonly skillUp: boolean;
}

export type PartySkillOutcome =
  | { readonly kind: 'none' }
  | { readonly kind: 'box'; readonly tid: number }
  | { readonly kind: 'line'; readonly tid: number; readonly args: readonly string[] }
  | { readonly kind: 'send'; readonly skill: number };

export function partySkillOutcome(skill: number, f: PartySkillFacts): PartySkillOutcome {
  if (!(f.size >= 2 && f.kind === 1) || !f.prop) return { kind: 'none' };
  if (!f.leader) return { kind: 'box', tid: TID_DIAG_0047 };
  if (f.level - f.prop.reqLevel < 0) return { kind: 'box', tid: TID_DIAG_0019 };
  if (f.point - f.prop.cost < 0) return { kind: 'box', tid: TID_DIAG_0049 };
  if (!f.running || f.skillUp) return { kind: 'send', skill };
  if (skill !== ST.CALL && skill !== ST.BLITZ && skill !== ST.RETREAT) {
    return { kind: 'line', tid: TID_GAME_NOT_TIME, args: [f.prop.name] };
  }
  return { kind: 'none' };
}

/** The state of one skill cell of the skill tab: usable, too expensive (points) or locked (party level / kind). */
export type SkillCellState = 'usable' | 'noPoints' | 'locked';

/**
 * `CWndPartySkill::OnDraw`: a normal party, or a party below the skill's level, is locked (name red, icon dimmed); an advanced party that
 * lacks the points shows the name in blue; else the skill can be used (black).
 */
export function skillCellState(
  kind: number,
  partyLevel: number,
  point: number,
  reqLevel: number,
  cost: number,
): SkillCellState {
  if (kind !== 0 && partyLevel - reqLevel >= 0) return point - cost >= 0 ? 'usable' : 'noPoints';
  return 'locked';
}
