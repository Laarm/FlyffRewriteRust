// The read model of the skill tree window (WP-CFEAT-31): `buildTreeView` turns the store state, the character and the skill rows into the
// `SkillTreeView` that `APP_SKILL3` polls; `createTreeEnv` is the data side of the editing rules (`model.ts`): the row of a skill and the
// points one level costs - from the wasm rules (`skillPointCost`, the rule the server prices with) once the module is there, from the
// `tables` constants (`vag_sp`, `expert_sp`, `pro_sp`) before.

import type { ClientDataIndex, ClientItem } from '../../../data/clientData';
import { JOB_KNIGHT_MASTER, JTYPE_BASE, JTYPE_COMMON, JTYPE_EXPERT } from '../../../gen/defs/job';
import type { RulesFacade } from '../../../rules';
import type { SkillTreeSlot, SkillTreeView } from '../../../ui/windows/skills/ports';
import { jobPanel } from './jobPanels';
import {
  buttonState,
  checkSkill,
  isEmptySlot,
  type SkillRule,
  type TreeEnv,
  type TreePlayer,
  type TreeState,
} from './model';
import type { SkillTreeState } from './store';

/** The tree-relevant columns of a skill row. */
export function ruleOf(row: ClientItem): SkillRule {
  return {
    kind1: row.item_kind1,
    expertMax: row.expert_max,
    reqDisLv: row.req_dis_lv,
    reSkill1: row.re_skill1,
    reSkillLevel1: row.re_skill_level1,
    reSkill2: row.re_skill2,
    reSkillLevel2: row.re_skill_level2,
    log: row.log,
  };
}

export interface TreeEnvDeps {
  data(): ClientDataIndex | null;
  /** The rules module, `null` before it is built. */
  rules(): RulesFacade | null;
}

/** The default costs per level (`Constant.inc`: `vag_sp`, `expert_sp`, `pro_sp`). */
const DEFAULT_COSTS = [1, 2, 3] as const;

/** `CProject::GetSkillPoint(prop)`: by the tier of the skill, the third value for every tier but the vagrant and the expert one. */
export function costByKind(kind1: number, costs: readonly [number, number, number]): number {
  if (kind1 === JTYPE_BASE) return costs[0];
  if (kind1 === JTYPE_EXPERT) return costs[1];
  return costs[2];
}

export function createTreeEnv(deps: TreeEnvDeps): TreeEnv {
  const memo = new Map<number, number>();
  const costsOf = (): readonly [number, number, number] => {
    const c = deps.data()?.bundle('tables')?.constants;
    return c ? [c.vag_sp, c.expert_sp, c.pro_sp] : DEFAULT_COSTS;
  };
  return {
    rule: (id) => {
      const row = deps.data()?.skill(id);
      return row ? ruleOf(row) : undefined;
    },
    cost: (id) => {
      const hit = memo.get(id);
      if (hit !== undefined) return hit;
      const row = deps.data()?.skill(id);
      if (!row) return DEFAULT_COSTS[2];
      let cost: number | undefined;
      const rules = deps.rules();
      if (rules) {
        try {
          cost = rules.skillPointCost(id);
        } catch {
          cost = undefined;
        }
      }
      const value = cost ?? costByKind(row.item_kind1, costsOf());
      // only an answer of the rules module is final; the constants may be replaced when the module arrives
      if (cost !== undefined) memo.set(id, value);
      return value;
    },
  };
}

/** The character facts the rules read: the master / hero tiers of the job ids (`IsMaster`, `IsHero`). */
export function playerOf(job: number, level: number): TreePlayer {
  return { level, master: job >= JOB_KNIGHT_MASTER && job < 24, hero: job >= 24 && job < 32 };
}

/** `m_bLegend`: the character is a master or a hero (`GetLegendChar() >= LEGEND_CLASS_MASTER`). */
export function isLegend(job: number): boolean {
  return job >= JOB_KNIGHT_MASTER && job < 32;
}

export interface ViewInput {
  readonly state: SkillTreeState;
  readonly job: number;
  readonly level: number;
  readonly data: ClientDataIndex | null;
  readonly env: TreeEnv;
}

/** The skills a window can draw: a known prop, not an event ("common") skill. */
function isDrawable(row: ClientItem | undefined): row is ClientItem {
  return row !== undefined && row.item_kind1 !== JTYPE_COMMON;
}

/** `SkillTreeView` of the current state. */
export function buildTreeView(i: ViewInput): SkillTreeView {
  const t: TreeState = i.state.tree;
  const player = playerOf(i.job, i.level);
  const panel = jobPanel(i.job);
  const jobName = (id: number): string => (id < 0 ? '' : (i.data?.job(id)?.name ?? ''));
  const slots: SkillTreeSlot[] = [];
  t.local.forEach((slot, index) => {
    if (isEmptySlot(slot)) return;
    const row = i.data?.skill(slot.skillId);
    if (!isDrawable(row)) return;
    slots.push({
      index,
      skillId: slot.skillId,
      kind: row.item_kind1,
      icon: row.icon === '' ? null : `icon/${row.icon}`,
      level: slot.level,
      serverLevel: t.server[index]?.level ?? 0,
      maxLevel: row.expert_max,
      learnable: checkSkill(t.local, index, i.env, player),
    });
  });
  const hero = panel?.hero ?? null;
  return {
    job: i.job,
    legend: isLegend(i.job),
    titles: [
      jobName(panel?.titles[0] ?? -1),
      jobName(panel?.titles[1] ?? -1),
      jobName(panel?.titles[2] ?? -1),
    ],
    panels: [panel?.panels[0] ?? null, panel?.panels[1] ?? null],
    // masters have no hero picture: the master skill starts at level 1
    heroBackground: i.job >= 24 ? hero : null,
    slots,
    sp: t.localSp,
    serverSp: t.serverSp,
    selected: t.selected,
    buttons: i.state.busy
      ? { add: false, sub: false, reset: false, finish: false }
      : buttonState(t, i.env),
    level: i.level,
  };
}
