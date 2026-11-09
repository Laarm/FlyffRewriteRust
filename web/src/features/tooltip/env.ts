// The `ComposeEnv` of a client context (Plan/45 §10.1): the texts, the item and skill props, the options and the small lookups a
// composition needs, read lazily from the bundles (`ClientDataIndex`) and the live stores. Tests build a `ComposeEnv` by hand.

import { RowLookup } from '../../data/lookup';
import type { ClientDataIndex } from '../../data/clientData';
import type { AddSkillProp } from '../../gen/bundles';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';
import { jobTypeByLayout, isInteriorityJob } from './jobs';
import type { AddSkillRow, ComposeEnv, Features } from './model';

/** What the env reads besides the data index (all late-bound: a context builds them before the stores are ready). */
export interface ComposeEnvDeps {
  readonly data: () => ClientDataIndex | null | undefined;
  readonly features: () => ClientFeatureConfig | undefined;
  /** `g_Option.m_nToolTipText`. */
  readonly colorSet: () => number;
  /** Wall-clock unix seconds. */
  readonly nowSec: () => number;
  readonly guildName?: ((id: number) => string | null) | undefined;
  readonly playerName?: ((id: number) => string | null) | undefined;
  readonly cooldownEnd?: ((group: number) => number | null) | undefined;
  readonly feedCount?: (() => number) | undefined;
  readonly feedPocketActive?: (() => boolean) | undefined;
  readonly needVisState?: ComposeEnv['needVisState'];
  readonly visActive?: ComposeEnv['visActive'];
}

/** `prj.GetAddSkillProp` over the `skills.levels` table: `idx = sub + max(level, 1) - 1`, raised by 5 below 5 (KEEP-QUIRK). */
export function addSkillIndex(subDefine: number, level: number): number {
  let idx = subDefine + Math.max(level, 1) - 1;
  if (idx < 5) idx += 5;
  return idx;
}

export function createComposeEnv(deps: ComposeEnvDeps): ComposeEnv {
  let levels: { version: number; lookup: RowLookup<AddSkillProp> | undefined } | null = null;
  const addSkillLookup = (): RowLookup<AddSkillProp> | undefined => {
    const d = deps.data();
    if (!d) return undefined;
    if (levels?.version !== d.version) {
      const table = d.bundle('skills')?.levels;
      levels = {
        version: d.version,
        lookup: table ? new RowLookup<AddSkillProp>(table) : undefined,
      };
    }
    return levels.lookup;
  };
  const FEATURE_DEFAULT: Features = {
    max_awake_color: false,
    item_count_tooltip: false,
    remove_max_recover_rate: false,
    better_armor_bonus: false,
    improved_ulti_gems: false,
    only_armor_awakes: false,
    no_pet_life: false,
  };
  return {
    text: (tid) => deps.data()?.text(tid) ?? '',
    textColor: (tid) => deps.data()?.textColor(tid) ?? 0xffffffff,
    prop: (id) => deps.data()?.item(id),
    skillProp: (id) => deps.data()?.skill(id),
    addSkill: (sub, level): AddSkillRow | undefined =>
      addSkillLookup()?.get(addSkillIndex(sub, level)),
    features: () => {
      const f = deps.features()?.features;
      if (!f) return FEATURE_DEFAULT;
      return {
        max_awake_color: f.max_awake_color,
        item_count_tooltip: f.item_count_tooltip,
        remove_max_recover_rate: f.remove_max_recover_rate,
        better_armor_bonus: f.better_armor_bonus,
        improved_ulti_gems: f.improved_ulti_gems,
        only_armor_awakes: f.only_armor_awakes,
        no_pet_life: f.no_pet_life,
      };
    },
    colorSet: deps.colorSet,
    nowSec: deps.nowSec,
    guildName: (id) => deps.guildName?.(id) ?? null,
    playerName: (id) => deps.playerName?.(id) ?? null,
    cooldownEnd: (g) => deps.cooldownEnd?.(g) ?? null,
    moverName: (index) => deps.data()?.mover(index)?.name ?? null,
    petTables: () => deps.data()?.bundle('item-ext')?.pet,
    feedCount: () => deps.feedCount?.() ?? 0,
    feedPocketActive: () => deps.feedPocketActive?.() ?? false,
    jobName: (job) => deps.data()?.job(job)?.name ?? '',
    isInteriorityJob,
    accessoryDsts: (itemId, level) => {
      const def = deps
        .data()
        ?.bundle('item-ext')
        ?.accessory.find((a) => a.item === itemId);
      if (!def) return [];
      const l = Math.min(Math.max(Math.trunc(level), 0), def.levels.length - 1);
      return (def.levels[l] ?? []).map(([dst, adj]) => [dst, adj] as const);
    },
    ...(deps.visActive ? { visActive: deps.visActive } : {}),
    ...(deps.needVisState ? { needVisState: deps.needVisState } : {}),
  };
}

export { jobTypeByLayout };
