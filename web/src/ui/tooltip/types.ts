// The types of the tooltip host (Plan/42 §9.1, §9.4, §9.8). The composers that turn an item or a skill into text are
// registered by 45 (WP-CFEAT-16/17); the host only needs the shapes below.

import type { RichRun } from '../text/markup';

/**
 * The part of an item the host reads: the name line (the default composer prints it), `kind2` (`dwItemKind2`: which items
 * are compared with the worn ones, §9.3) and `parts` (`dwParts`: which slot the comparison looks at). 45's richer item
 * record extends it (the composers cast `ItemView` to their record; the host never looks further).
 */
export interface ItemView {
  name: string;
  kind2: number;
  parts: number;
  /**
   * A key that names this item instance (the serial, or the object id): the composed tooltip is cached per
   * `(serial, ctx.version)`. Items without one are composed on every hover.
   */
  serial?: number | string;
}

/**
 * The numbers one tooltip needs (§9.6): a single batched wasm call per hover. `multiplier` is `GetItemMultiplier`, `ability`
 * the shown min/max (`int(min·f) + int(opt^1.5)`), `equipDenied` the `TID` of `equip_check` (0 = may wear;
 * `EQUIP_SILENT_REFUSAL` = refused without a text). The remaining members are filled by `item_tooltip_numbers` once it
 * exists (prices, decoded random options, sets, gems, element resist); a composer treats a missing member as "not shown".
 */
export interface ItemTooltipNumbers {
  readonly multiplier: number;
  readonly ability: { readonly min: number; readonly max: number };
  readonly equipDenied: number;
  readonly prices?: {
    readonly buyUnit: number;
    readonly buyTax: number;
    readonly sellUnit: number;
    readonly sellTax: number;
  };
  /**
   * The full answer of `item_tooltip_numbers` (random options, set summary, gem and piercing totals, prices with the chip cost): the
   * host never looks inside; 45's composer reads it as `rules/types ItemTooltipNumbers`.
   */
  readonly detail?: unknown;
}

/**
 * Computes the numbers of an item (the wasm side). `null` = no rules context yet: the composer then prints what needs no
 * number. Called AT MOST ONCE per composition.
 */
export type ItemNumbersProvider = (
  item: ItemView,
  ctx: ItemTooltipCtx,
) => ItemTooltipNumbers | null;

/** What a composer gets besides the item (`ItemTooltipCtx`, §9.4; 45 adds its own fields on top). */
export interface ItemTooltipCtx {
  /** The APP id of the source window (`APP_INVENTORY`, `APP_SHOPEX`, `APP_QUERYEQUIP`, ...). */
  flag: number;
  /** `performance.now()` ms (cooldown and keep-time display). */
  now: number;
  /** The item worn in a slot (`GetEquipItem(dwParts)`): the comparison tooltips of §9.3 read it. */
  equippedAt?: (parts: number) => ItemView | null;
  /**
   * The cache key besides the item serial: the owner's equipment version and the shop context (a changed equipment or a
   * shop opening changes the lines of a tooltip). Compose-time only; the caller bumps it.
   */
  version?: number | string;
  /** The numbers of this item (filled by the host from the numbers provider, once per composition). */
  numbers?: ItemTooltipNumbers | null;
  /** The shop context / owner / features of 45 (§9.4): opaque to the host. */
  extra?: unknown;
}

/** An ultimate weapon's extras: the icon at the top-left, the gem slots at the bottom (§9.3). */
export interface UltimateBand {
  /** The icon file (`Icon/Icon_Ultimate.dds`). */
  icon: string;
  /** The number of gem slots drawn along the bottom. */
  slots: number;
  /** The icons of the gems in the slots (`Item/...`), `null` for an empty slot. */
  gems?: readonly (string | null)[];
}

/** The composed text of one tooltip. */
export interface TooltipContent {
  runs: RichRun[];
  ultimate?: UltimateBand;
  /**
   * The text depends on the clock (a cooldown, a keep time): the host re-composes it once a second while it is shown and never
   * serves it from the cache.
   */
  volatile?: boolean;
}

export type ItemTooltipComposer = (item: ItemView, ctx: ItemTooltipCtx) => TooltipContent;
/** `PutToolTip_Skill`: `null` = no tooltip (a missing skill prop). */
export type SkillTooltipComposer = (
  skillId: number,
  level: number,
  active: boolean,
) => TooltipContent | null;
/** `PutToolTip_Troupe`. */
export type TroupeTooltipComposer = (partySkillId: number) => TooltipContent | null;

export interface TooltipComposers {
  item: ItemTooltipComposer;
  skill: SkillTooltipComposer;
  troupe: TroupeTooltipComposer;
}

/** Optional extras of a text tooltip. */
export interface TooltipExtra {
  ultimate?: UltimateBand;
}
