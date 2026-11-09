// The numbers plumbing of the item tooltip host (Plan/42 §9.6, WP-UI-20): an `ItemNumbersProvider` over the wasm exports that
// exist today (`item_multiplier`, `equip_check`) until the batched `item_tooltip_numbers` export lands (§25 OQ8; then the provider
// is that one call). The host calls the provider at most once per composition.
//
// `ability` is the shown attack / defence range `int(min * f) + int(opt ^ 1.5)` of `PutItemMinMax` (WndManager.cpp): `f` is
// `GetItemMultiplier` (a `float`), `min` / `max` the base range of the item prop and `opt` its upgrade level. KEEP: the product is
// taken in single precision (`Math.fround`) like the original's `float`.

import type { ItemView } from './types';
import type { ItemNumbersProvider, ItemTooltipNumbers } from './types';

/** The exports of `RulesFacade` the provider needs (structurally typed: `ui/` does not import `rules/`). */
export interface RulesNumbersSource {
  itemMultiplier(itemId: number, abilityOption: number, expired: boolean): number;
  equipCheck(
    itemId: number,
    randomOptHi: number,
    sex: number,
    job: number,
    level: number,
    rweaponItem: number,
  ): number;
}

/** What the feature layer knows about the item of a tooltip. */
export interface ItemNumbersInput {
  readonly itemId: number;
  /** `GetAbilityOption()`: the upgrade level (+N). */
  readonly abilityOption: number;
  readonly expired: boolean;
  /** The high word of the item's random option, `equip_check`'s `randomOptHi`. */
  readonly randomOptHi: number;
  /** `dwAbilityMin` / `dwAbilityMax` of the prop (attack or defence range). */
  readonly abilityMin: number;
  readonly abilityMax: number;
}

/** The wearer: `equip_check` compares the item with sex, job, level and the right-hand weapon (arrows need a bow). */
export interface WearerInput {
  readonly sex: number;
  readonly job: number;
  readonly level: number;
  readonly rweaponItem: number;
}

/** `int(opt ^ 1.5)`: exact for the perfect squares (`4 -> 8`, `9 -> 27`), truncated for the rest. */
export function upgradeBonus(opt: number): number {
  if (opt <= 0) return 0;
  const root = Math.round(Math.sqrt(opt));
  if (root * root === opt) return root * root * root;
  return Math.trunc(opt * Math.sqrt(opt));
}

/** `int(base * f) + int(opt ^ 1.5)` with the single-precision product of the original. */
export function abilityValue(base: number, multiplier: number, opt: number): number {
  return Math.trunc(Math.fround(base * Math.fround(multiplier))) + upgradeBonus(opt);
}

export interface RulesNumbersOptions {
  readonly rules: RulesNumbersSource;
  /** The item record behind a tooltip item (`null`: no numbers, the composer prints what needs none). */
  readonly input: (item: ItemView) => ItemNumbersInput | null;
  /** The wearer; `null` before the character is known. */
  readonly wearer: () => WearerInput | null;
}

/** One `item_multiplier` and one `equip_check` call per composition. */
export function createRulesNumbersProvider(o: RulesNumbersOptions): ItemNumbersProvider {
  return (item): ItemTooltipNumbers | null => {
    const it = o.input(item);
    if (!it) return null;
    const multiplier = o.rules.itemMultiplier(it.itemId, it.abilityOption, it.expired);
    const w = o.wearer();
    const equipDenied = w
      ? o.rules.equipCheck(it.itemId, it.randomOptHi, w.sex, w.job, w.level, w.rweaponItem)
      : 0;
    return {
      multiplier,
      ability: {
        min: abilityValue(it.abilityMin, multiplier, it.abilityOption),
        max: abilityValue(it.abilityMax, multiplier, it.abilityOption),
      },
      equipDenied,
    };
  };
}
