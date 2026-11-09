// The rates of the tax (`src:_Common/Tax.cpp:95-163`; Plan/45 §28.3; WP-CFEAT-52): the pure helpers of the tax slice. Every window prints the INTEGER percent of the slice; the fraction handed to
// the price rules is the original's two f32 multiplies, bit for bit the server's (`percent_factor`).

import {
  II_CHP_RED,
  II_SYS_SYS_SCR_AWAKE,
  II_SYS_SYS_SCR_PERIN,
  II_SYS_SYS_SCR_PETAWAKE,
} from '../../../gen/defs/item';
import type { TaxAllInfo } from '../../../gen/proto/flyff/v1/lord_pb';
import type { TaxContinentRow, TaxSlice } from '../store';

/** `TaxKind`: sales 0, purchase 1, admission 2 (the Heaven Tower fee). */
export const TAX_KIND = { Sales: 0, Purchase: 1, Admission: 2 } as const;

/** `NULL_ID` of the wire: no owner. */
export const TAX_NO_OWNER = 0xffff_ffff;

/** The f32 of `0.1f`. */
const F_TENTH = Math.fround(0.1);

/**
 * `(float) percent * 0.1f * 0.1f`: the rate as the fraction the prices use (`Tax.cpp:101-139`): two f32 multiplies, so 19 % is `0.19000001` and a double product would differ.
 * A product of an integer and an f32 is exact in a double, so rounding it to f32 once per multiply is the f32 multiply.
 */
export function taxFraction(percent: number): number {
  return Math.fround(Math.fround(percent * F_TENTH) * F_TENTH);
}

/** The Perin, the Red Chip and the two awakening scrolls pay no tax (`CTax::IsApplyTaxRate`, `Tax.cpp:146-163`; the twin of `flyff_rules::item::is_taxable_item`). */
export function isTaxableItem(itemId: number): boolean {
  return (
    itemId !== II_SYS_SYS_SCR_PERIN &&
    itemId !== II_CHP_RED &&
    itemId !== II_SYS_SYS_SCR_AWAKE &&
    itemId !== II_SYS_SYS_SCR_PETAWAKE
  );
}

/** The slice of a `TaxAllInfo` (the owner `0xFFFFFFFF` is none). */
export function sliceOfAllInfo(m: TaxAllInfo): TaxSlice {
  return {
    continents: m.continents.map((c): TaxContinentRow => ({
      cont: c.continent,
      owner: c.ownerId === TAX_NO_OWNER ? null : c.ownerId,
      rates: new Map(c.rates.map((r) => [r.kind, r.rate])),
    })),
  };
}

/** The integer percent of a kind of a continent, 0 when absent. */
export function rateOf(slice: TaxSlice, cont: number, kind: number): number {
  return slice.continents.find((c) => c.cont === cont)?.rates.get(kind) ?? 0;
}

/** The owner guild of a continent, `null` for none. */
export function ownerOf(slice: TaxSlice, cont: number): number | null {
  return slice.continents.find((c) => c.cont === cont)?.owner ?? null;
}

/** The presets of the rate window: `max( current, min )` (KEEP: not clamped to the maximum). */
export function ratePreset(current: number, min: number): number {
  return current < min ? min : current;
}
