// The ranges of the item builder (Plan/25 section 5.2, Plan/19 section 2): what the form allows before the server is asked. They
// equal the server's (`GET /admin/api/items/catalog-meta` `limits`, `crates/flyff-server/tests/golden/admin/item_limits.json`);
// `ranges.test.ts` holds this file to the same golden, and the form switches to the server's numbers as soon as the metadata
// arrived (`useCatalog`), so a change on the server never leaves the form refusing or accepting the wrong thing for long.
import type { ItemLimits } from '../gen/dto';

export const DEFAULT_LIMITS: ItemLimits = {
  count_max: 9999,
  upgrade_gear: 10,
  upgrade_accessory: 20,
  upgrade_collector: 5,
  upgrade_ultimate: 10,
  element_level: 20,
  option_lines: 3,
  option_dst_max: 127,
  option_adj_max: 511,
  piercing_suit: 4,
  piercing_weapon: 10,
  ultimate_gems: 5,
  vis: 9,
  pet_exp_egg: 50_000,
  pet_exp: 100_000,
  pet_life: 99,
  pet_energy: [1, 180, 300, 420, 600, 1500],
  text_bytes: 31,
  pet_name_bytes: 16,
  basket_lines: 50,
  recipients: 500,
  gold_max: 2_147_483_647,
  mailbox: 50,
};

/** The elements of `SAI79::ePropType` (1..5) in game wording. */
export const ELEMENTS: readonly { readonly id: number; readonly name: string }[] = [
  { id: 1, name: 'Fire' },
  { id: 2, name: 'Water' },
  { id: 3, name: 'Electricity' },
  { id: 4, name: 'Wind' },
  { id: 5, name: 'Earth' },
];

/** The wire name of an element id (the `Element` enum of `ItemSpec`). */
export const ELEMENT_KEYS = ['fire', 'water', 'electricity', 'wind', 'earth'] as const;

/** The pet kinds (`CPet::m_nKind`, 4..6 have no data). */
export const PET_KINDS: readonly string[] = [
  'Tiger (STR)',
  'Lion (STA)',
  'Rabbit (DEX)',
  'Fox (INT)',
];

/** The pet levels (`PL_*`). */
export const PET_LEVELS: readonly string[] = ['Egg', 'D', 'C', 'B', 'A', 'S'];
