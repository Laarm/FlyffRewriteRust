// The inputs of the item tooltip composer (Plan/45 §10.1, Plan/42 §9.4): the item instance, the per-hover context and the services
// a composition needs. The composer is a pure function of these (it calls no wasm function: the one batched
// `item_tooltip_numbers` result arrives in `numbers`), so the golden tests build them by hand.

import type { ItemTextColors } from './colors';
import type { RichBuilder } from './rich';
import type { ClientItem } from '../../data/clientData';
import type { ItemExtBundle, SetItemAvail } from '../../gen/bundles';
import type { ItemTooltipNumbers } from '../../rules/types';
import type { ItemView } from '../../ui/tooltip/types';

/** The item instance a tooltip is about (a structural subset of `InvItem`, so every window's item record qualifies). */
export interface TooltipItemRecord {
  /** The object id in its container (set by inventory items; used to tell whether the item is worn). */
  readonly objId?: number | undefined;
  readonly itemId: number;
  /** The stack count (`m_nItemNum`). */
  readonly num: number;
  /** `m_byFlag` (`ITEM_FLAG`: expired 1, binds 2, using 4). */
  readonly flag: number;
  /** `GetAbilityOption()`: the +N upgrade level. */
  readonly abilityOption: number;
  readonly idGuild: number;
  /** `m_bItemResist`: the element (`SAI79::ePropType`). */
  readonly itemResist: number;
  readonly resistAbilityOption: number;
  /** `m_nResistSMItemId`: the cash element stone item, 0 = none. */
  readonly resistSmItemId: number;
  /** The normal sockets (card item ids, 0 = empty); for a vis pet the vis items. */
  readonly cards: readonly number[];
  /** The ultimate sockets (gem item ids). */
  readonly gems: readonly number[];
  /** The vis keep times (unix seconds). */
  readonly visExpireAt: readonly number[];
  /** The packed 64-bit random-option word. */
  readonly randomOpt: bigint;
  /** Unix seconds, 0 = none (`m_dwKeepTime`). */
  readonly keepTimeAt: number;
  /** `m_szItemText` (the sealed character's name). */
  readonly itemText: string;
  /** `m_nRepair`: for a seal scroll the job. */
  readonly repair: number;
  /** `m_nRepairNumber`: for a seal scroll the level. */
  readonly repairNumber: number;
  readonly pet?: PetRecord | undefined;
  readonly transformVisPet?: boolean | undefined;
}

/** `CPet` of an egg / pet item. */
export interface PetRecord {
  readonly kind: number;
  /** `PL_EGG` 0 .. `PL_S` 5. */
  readonly level: number;
  readonly exp: number;
  readonly energy: number;
  readonly life: number;
  readonly availLevel: readonly number[];
  readonly name: string;
}

/** The item view the composer reads (`ItemView` + the instance). Equipment views of other windows build the same shape. */
export interface TooltipItemView extends ItemView {
  readonly item: TooltipItemRecord;
}

export function isTooltipItemView(v: ItemView): v is TooltipItemView {
  const item: unknown = (v as Partial<TooltipItemView>).item;
  return typeof item === 'object' && item !== null;
}

/** The mover the requirement lines are evaluated for (the own player, or the inspected one for `APP_QUERYEQUIP`). */
export interface TooltipOwnerInfo {
  /** 0 male, 1 female. */
  readonly sex: number;
  readonly job: number;
  readonly level: number;
  /** `JTYPE_*` of the job (`GetJobType()`), for the master/hero level rule. */
  readonly jobType: number;
}

/** The shop window a price line belongs to. */
export interface ShopTooltipContext {
  /** `m_nVenderType`: 0 penya, 1 red chips. */
  readonly vendorType: 0 | 1;
  /** `CTax::IsApplyTaxRate` holds for the viewer (a continent with a tax rate table, `CONT_NODATA` pays none). */
  readonly taxApplies: boolean;
}

/** The per-hover context of 45 (`ItemTooltipCtx.extra`). */
export interface ItemTooltipExtra {
  /** The viewer; default: the own player. */
  readonly owner?: TooltipOwnerInfo | undefined;
  /** The shop window that is open: a buy price on `APP_SHOPEX`, a sell price on `APP_INVENTORY`. */
  readonly shop?: ShopTooltipContext | null | undefined;
  /** `m_nCost` of a personal-vendor entry (`APP_VENDOR_REVISION`). */
  readonly vendorPrice?: number | null | undefined;
  /** The item is worn by the viewer (set-enchant bonus). */
  readonly worn?: boolean | undefined;
  /** The armour +N set bonus row to print (`PutEnchantOpt`), resolved by the module from the worn pieces. */
  readonly armorSetBonus?: SetItemAvail | null | undefined;
}

/** The rule the module resolves: is a card a plain sub-tooltip header (`PutEquipItemText`)? Kept for the host. */
export interface Features {
  readonly max_awake_color: boolean;
  readonly item_count_tooltip: boolean;
  readonly remove_max_recover_rate: boolean;
  readonly better_armor_bonus: boolean;
  readonly improved_ulti_gems: boolean;
  readonly only_armor_awakes: boolean;
  readonly no_pet_life: boolean;
}

/** Everything a composition reads besides the item. */
export interface ComposeEnv {
  /** `prj.GetText`. */
  readonly text: (tid: number) => string;
  /** `prj.GetTextColor`. */
  readonly textColor: (tid: number) => number;
  /** `prj.GetItemProp`. */
  readonly prop: (itemId: number) => ClientItem | undefined;
  /** `prj.GetSkillProp`. */
  readonly skillProp: (skillId: number) => ClientItem | undefined;
  /** `prj.GetAddSkillProp(subDefine, level)` (the `idx < 5` quirk included). */
  readonly addSkill: (subDefine: number, level: number) => AddSkillRow | undefined;
  readonly features: () => Features;
  /** `g_Option.m_nToolTipText`. */
  readonly colorSet: () => number;
  /** `time_null()`: wall-clock unix seconds. */
  readonly nowSec: () => number;
  /** `g_GuildMng.GetGuild(id)->m_szGuild`. */
  readonly guildName: (idGuild: number) => string | null;
  /** `CPlayerDataCenter::GetPlayerString(id)` (the couple ring). */
  readonly playerName: (idPlayer: number) => string | null;
  /** The end of the cooldown of a group on the client clock (ms), `null` when none runs. */
  readonly cooldownEnd: (group: number) => number | null;
  /** The mover prop of a pet index (`prj.GetMoverProp`): the cage name of an egg. */
  readonly moverName: (index: number) => string | null;
  /** The pet tables of the `item-ext` bundle (`CPetProperty`). */
  readonly petTables: () => ItemExtBundle['pet'] | undefined;
  /** `GetItemNumForClient(II_SYS_SYS_FEED_01)`. */
  readonly feedCount: () => number;
  /** The feed pocket buff is active (`HasBuff(BUFF_ITEM, II_SYS_SYS_SCR_PET_FEED_POCKET)`). */
  readonly feedPocketActive: () => boolean;
  /** `job name` of the seal scroll (`prj.m_aJob[job].szName`). */
  readonly jobName: (job: number) => string;
  /** `IsInteriorityJob(own, itemJob)`. */
  readonly isInteriorityJob: (ownJob: number, itemJob: number) => boolean;
  /** `CAccessoryProperty::GetDst(item, level)`: the `(dst, adj)` bonuses of an accessory, `[]` for any other item. */
  readonly accessoryDsts: (
    itemId: number,
    level: number,
  ) => readonly (readonly [dst: number, adj: number])[];
  /** `IsSatisfyNeedVis(pet, vis)` is true for a vis of the pet (default: every socketed vis counts as active). */
  readonly visActive?: (pet: TooltipItemRecord, visItemId: number) => boolean;
  /** `IsSatisfyNeedVis(activePet, vis)` for the inventory vis: which needed vis are missing (default: both, no buff pet). */
  readonly needVisState?: (vis: ClientItem) => 'ok' | 'first' | 'second' | 'both';
}

/** The fields of an `AddSkillProp` row the tooltips read. */
export interface AddSkillRow {
  readonly dest_param: readonly [number, number];
  readonly adj_param_val: readonly [number, number];
}

/** What the section functions get. */
export interface ComposeCx {
  readonly view: TooltipItemView;
  readonly item: TooltipItemRecord;
  readonly prop: ClientItem;
  /** `ctx.flag`: the APP id of the source window. */
  readonly flag: number;
  /** `ctx.now`: the client clock in ms. */
  readonly now: number;
  readonly numbers: ItemTooltipNumbers | null;
  readonly extra: ItemTooltipExtra;
  readonly owner: TooltipOwnerInfo;
  readonly env: ComposeEnv;
  readonly colors: ItemTextColors;
  readonly b: RichBuilder;
  /** Set to true by a section whose text depends on the clock (cooldown, keep time): the host refreshes it. */
  volatile: boolean;
}
