// Using an item from the inventory (Plan/45 §9; ref/99 §1, §4.2.1; WP-CFEAT-10, -13, -14): the double click on the bag, the taskbar ITEM
// shortcut (`items.useFromShortcut`, F1-F9), the upgrade cursor and the item-use router R0-R31 are the `ItemDispatch` of
// `../dispatch`; `ItemUse` is the name the inventory feature has always exported it under.

export { ItemDispatch as ItemUse } from '../dispatch';
export type { DispatchEnv as UseEnv } from '../dispatch';
