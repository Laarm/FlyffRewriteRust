// The skill tooltip (`CWndMgr::PutToolTip_Skill`, `src:_Interface/WndManager.cpp:4265-4380`; Plan/42 §9.8, 45 §10.5, ref/99 §7 #3;
// WP-CFEAT-31): name (and level while the skill is learnable), the MP and FP cost of the skill level, the prerequisites that the
// player owns - red while the player's level of them is too low -, the required character level (red below it; masters and heroes
// are exempt), the description and the remaining reuse delay. A pure composer over the skill rows, the add-skill rows and a few
// facts of the player, registered with the tooltip host (`ui.tooltip.registerComposers({ skill })`).

import type { AddSkillProp } from '../../../gen/bundles';
import type { ClientDataIndex } from '../../../data/clientData';
import { RowLookup } from '../../../data/lookup';
import { DST_FP_DEC_RATE, DST_MP_DEC_RATE, DST_SKILL_LEVEL } from '../../../gen/defs/attribute';
import {
  TID_GAME_BASESKILLLEVEL,
  TID_GAME_SKILLDIS,
  TID_TOOLTIP_COOLTIME,
} from '../../../gen/defs/text';
import { parseMarkup } from '../../../ui/text/markup';
import type { SkillTooltipComposer } from '../../../ui/tooltip/types';
import { formatTemplate } from '../../common/text';
import { NULL_ID, type SkillTable } from './model';

/** The facts of the player the tooltip reads. */
export interface SkillTooltipDeps {
  data(): ClientDataIndex | null;
  /** `g_pPlayer->GetLevel()`. */
  level(): number;
  /** `IsMaster() || IsHero()`: the level requirement of a skill does not apply. */
  legend(): boolean;
  /** The server table (`g_pPlayer->m_aJobSkill`). */
  table(): SkillTable;
  /** `GetParam(dst, base)` of the own DST replica (`base + adj`, an override replaces it). */
  param(dst: number, base: number): number;
  /** `GetReuseDelay(slot)`: the remaining reuse delay of a skill in ms, `0` = ready. */
  reuseMs?(skillId: number): number;
}

/** `GetAddSkillProp(subDefine, level)`: the row of one skill level (`Project.cpp:912`). */
export function addSkillIndex(subDefine: number, level: number): number {
  const lv = level < 1 ? 1 : level;
  let index = subDefine + lv - 1;
  if (index < 5) index += 5;
  return index;
}

const lookups = new WeakMap<object, RowLookup<AddSkillProp>>();

/** The add-skill rows of a loaded `skills` bundle (the lookup is built once per bundle). */
export function addSkillLookup(data: ClientDataIndex | null): RowLookup<AddSkillProp> | undefined {
  const bundle = data?.bundle('skills');
  if (!bundle) return undefined;
  let l = lookups.get(bundle);
  if (!l) {
    l = new RowLookup<AddSkillProp>(bundle.levels);
    lookups.set(bundle, l);
  }
  return l;
}

/** `GetReqMp` / `GetReqFp`: the cost reduced by the DST rate (the sentinel 9 999 999 stays). */
export function reducedCost(cost: number, rate: number): number {
  if (cost === 9_999_999) return cost;
  return cost - Math.trunc((cost * rate) / 100);
}

/** `m:ss` split of the remaining delay for `TID_TOOLTIP_COOLTIME(minutes, seconds)` (`CTimeSpan(dwDelay / 1000)`). */
function minutesSeconds(ms: number): [number, number] {
  const total = Math.trunc(ms / 1000);
  return [Math.trunc(total / 60) % 60, total % 60];
}

const NAME_COLOR = '#b#cff2fbe6d';
const RED = '#cffff0000';

export function createSkillComposer(deps: SkillTooltipDeps): SkillTooltipComposer {
  return (skillId, level, active) => {
    const data = deps.data();
    const prop = data?.skill(skillId);
    if (!data || !prop) return null;
    const own = deps.table().find((s) => s.skillId === skillId);
    // `GetSkillLevel`: the level plus `DST_SKILL_LEVEL`, at most `dwExpertMax`
    const shown = own ? Math.min(deps.param(DST_SKILL_LEVEL, own.level), prop.expert_max) : level;
    const add = addSkillLookup(data)?.get(addSkillIndex(prop.sub_define, shown));
    if (!add) return null;
    const tid = (n: number, args: readonly (string | number)[]): string =>
      formatTemplate(data.text(n), args);
    let s = active
      ? `${NAME_COLOR}${prop.name}   Lv ${String(shown)}#nb#nc`
      : `${NAME_COLOR}${prop.name}#nb#nc`;
    if (add.req_mp >>> 0 !== NULL_ID) {
      s += `\nMP : ${String(reducedCost(add.req_mp, deps.param(DST_MP_DEC_RATE, 0)))}`;
    }
    if (add.req_fp >>> 0 !== NULL_ID) {
      s += `\nFP : ${String(reducedCost(add.req_fp, deps.param(DST_FP_DEC_RATE, 0)))}`;
    }
    // the prerequisites that the player owns; one below the needed level is red
    for (const [need, needLevel] of [
      [prop.re_skill1, prop.re_skill_level1],
      [prop.re_skill2, prop.re_skill_level2],
    ] as const) {
      if (need >>> 0 === NULL_ID) continue;
      const have = deps.table().find((x) => x.skillId === need);
      if (!have) continue;
      const name = data.skill(need)?.name ?? '';
      const line = tid(TID_GAME_SKILLDIS, [name, needLevel]);
      s += have.level < needLevel ? `\n${RED}${line}#nc` : `\n${line}#nc`;
    }
    const reqLine = tid(TID_GAME_BASESKILLLEVEL, [prop.req_dis_lv >>> 0]);
    // `(int)dwReqDisLV`: the NULL value is negative as an int
    s +=
      deps.level() < (prop.req_dis_lv | 0) && !deps.legend()
        ? `\n${RED}${reqLine}#nc`
        : `\n${reqLine}`;
    if (prop.description !== '=') s += `\n${prop.description}`;
    const delay = own ? (deps.reuseMs?.(skillId) ?? 0) : 0;
    if (delay > 0) {
      const [m, sec] = minutesSeconds(delay);
      s += `\n${tid(TID_TOOLTIP_COOLTIME, [m, sec])}`;
    }
    return { runs: parseMarkup(s), volatile: delay > 0 };
  };
}
