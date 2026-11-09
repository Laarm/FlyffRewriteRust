// `features/pockets` (WP-CFEAT-29, the pockets part): the backpack window and the moves between the bag and the pockets. See `pocketsModule.ts`.
export { pocketsModule, pocketsOf, type PocketsFeature } from './pocketsModule';
export { createPocketSenders, POCKET_BAG, type PocketSenders } from './senders';
export {
  applyAdd,
  applyAttribute,
  applyRemove,
  applySelf,
  ATTR_AVAIL,
  ATTR_EXPIRED,
  definePocketsStore,
  initialPocketsState,
  pocketAvailable,
  pocketCapacity,
  POCKET_COUNT,
  type PocketsState,
  type PocketsStore,
  type PocketState,
} from './state';
export { PocketViewBuilder, timeLeft, type PocketViewEnv } from './view';
