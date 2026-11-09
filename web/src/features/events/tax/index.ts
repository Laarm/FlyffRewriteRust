// `features/events/tax` (WP-CFEAT-52): the tax tables of the continents, the tax area of the player and the rate windows.
export { registerTax, taxOf } from './taxModule';
export type { TaxFeature } from './taxModule';
export { areaOf, continentOfMover, taxAreaOf, CONT_ALL, CONT_EAST, CONT_WEST } from './area';
export {
  isTaxableItem,
  ownerOf,
  ratePreset,
  rateOf,
  sliceOfAllInfo,
  taxFraction,
  TAX_KIND,
} from './rates';
export { taxSenders } from './senders';
export type { TaxSenders } from './senders';
