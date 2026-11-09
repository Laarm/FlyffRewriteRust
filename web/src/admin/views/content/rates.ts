// The rates of the World & Content page (Plan/25 section 7.1, 23 section 8.5): what each rate is, which layer of the configuration is in force
// (data default, flyff.toml, runtime override) and the maths of the slider and the number input, both held to the range the server tells
// (`min`, `max`, `min_exclusive` of `GET /admin/api/rates`). Pure.

/** A row of `GET /admin/api/rates`. */
export interface RateRow {
  readonly key: string;
  readonly config_key: string;
  readonly data_default: number;
  readonly toml: number | null;
  readonly overridden: number | null;
  readonly effective: number;
  readonly min: number | null;
  readonly max: number | null;
  readonly min_exclusive: boolean | null;
}

/** What the operator calls a rate and what it does. */
export const RATE_INFO: Readonly<
  Record<string, { readonly label: string; readonly hint: string }>
> = {
  exp: { label: 'Monster EXP', hint: 'Multiplies the experience a monster gives.' },
  drop: { label: 'Item drops', hint: 'Multiplies the chance that a monster drops an item.' },
  gold: { label: 'Penya drops', hint: 'Multiplies the penya a monster drops.' },
  hit: { label: 'Monster hit rate', hint: 'Multiplies how often monsters hit.' },
  shop: { label: 'Shop prices', hint: 'Multiplies what NPC shops charge (0.5 to 2).' },
  party: { label: 'Party EXP', hint: 'Multiplies the experience bonus of a party.' },
  aggressive: {
    label: 'Aggressive monsters',
    hint: 'The share of monsters that attack first (0 = none, 1 = all that can).',
  },
};

/** The smallest value an input offers for a range whose lower bound is excluded (`0 < v`). */
export const SMALLEST_POSITIVE = 0.01;

/** The lowest value the form accepts. */
export function lowest(r: RateRow): number {
  const min = r.min ?? 0;
  return r.min_exclusive === true ? Math.max(SMALLEST_POSITIVE, min) : min;
}

/** The highest value the form accepts. */
export function highest(r: RateRow): number {
  return r.max ?? 1000;
}

/** `true` when `v` may be set. */
export function allowed(r: RateRow, v: number): boolean {
  if (!Number.isFinite(v)) return false;
  const min = r.min ?? 0;
  const aboveMin = r.min_exclusive === true ? v > min : v >= min;
  return aboveMin && v <= highest(r);
}

/** `v` held to the range. */
export function clampRate(r: RateRow, v: number): number {
  if (!Number.isFinite(v)) return r.effective;
  return Math.min(highest(r), Math.max(lowest(r), v));
}

/** Rounds a rate to what it needs: two decimals below 10, one above. */
export function roundRate(v: number): number {
  if (v >= 100) return Math.round(v);
  if (v >= 10) return Math.round(v * 10) / 10;
  return Math.round(v * 100) / 100;
}

/** The slider has this many steps. */
export const SLIDER_STEPS = 1000;

/** A wide range (1000 times its lowest value or more) is laid out on a logarithmic slider, else on a linear one. */
export function isLog(r: RateRow): boolean {
  const lo = lowest(r);
  return lo > 0 && highest(r) / lo >= 50;
}

/** Where a value sits on the slider, `0..=SLIDER_STEPS`. */
export function toSlider(r: RateRow, v: number): number {
  const lo = lowest(r);
  const hi = highest(r);
  const c = clampRate(r, v);
  const f = isLog(r) ? Math.log(c / lo) / Math.log(hi / lo) : (c - lo) / (hi - lo);
  return Math.round(Math.min(1, Math.max(0, f)) * SLIDER_STEPS);
}

/** The value of a slider position (rounded, held to the range). */
export function fromSlider(r: RateRow, pos: number): number {
  const lo = lowest(r);
  const hi = highest(r);
  const f = Math.min(1, Math.max(0, pos / SLIDER_STEPS));
  const v = isLog(r) ? lo * Math.exp(f * Math.log(hi / lo)) : lo + f * (hi - lo);
  return clampRate(r, roundRate(v));
}

/** `3`, `2.5`, `0.25`: a rate as people write it. */
export function formatRate(v: number): string {
  if (Number.isInteger(v)) return String(v);
  return String(Number(v.toFixed(4)));
}

/** The layer that gives the effective value. */
export function layerOf(r: RateRow): 'override' | 'toml' | 'default' {
  if (r.overridden !== null) return 'override';
  if (r.toml !== null && r.toml !== r.data_default) return 'toml';
  return 'default';
}
