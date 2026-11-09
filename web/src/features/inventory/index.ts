// `features/inventory` (WP-CFEAT-10): the own bag and equipment, the inventory window's ports, the item-use path and the
// drop / delete flows. See `window/inventoryModule.ts`.
export { ItemContainer, NO_SLOT } from './container';
export type { StackItem, SwapOutcome } from './container';
export { itemFromWire, attrOption, ITEM_FLAG, UI, type InvItem } from './items';
export {
  applyAddItem,
  applyContainer,
  applyCooldown,
  applyCreateItem,
  applyEquip,
  applyExtra,
  applyHideCoat,
  applyMoveItem,
  applyUpdateItem,
  applyUpdateItemEx,
  cooldownGroup,
  defineInventoryStore,
  initialInventoryState,
  type InventoryState,
  type InventoryStore,
  type ItemRules,
} from './state';
export { InventoryViewBuilder, type InvItemView } from './view';
export { inventoryModule, inventoryOf, type InventoryFeature } from './window/inventoryModule';
export { ItemUse, type UseEnv } from './window/use';
export {
  ItemDispatch,
  ItemRouter,
  UpgradeCursor,
  type Decision,
  type DispatchEnv,
  type DispatchSenders,
  type DispatchWindows,
  type SelfFacts,
} from './dispatch';
export { createDispatchSenders, type ItemUseSenders } from './dispatch/senders';
export { dropOnTrash, dropOnWorld, type DropEnv } from './drop/drop';
export { createInventorySenders, itemData, type InventorySenders } from './senders';
