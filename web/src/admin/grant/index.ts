// The grant UI (Plan/25 section 5.3; WP-ADMIN-21): ONE component, usable as the Item builder tab and as a modal dialog.
//
//     import { openGrantDialog } from '../grant';
//     openGrantDialog({ recipients: [{ id: 1234, name: 'Leadhero', online: true }] });     // the player rows, the drawer
//     openGrantDialog({ ground: { world: 1, x: 6960, y: 100, z: 3274 } });                  // the map inspector: drop on the ground
//     openGrantDialog({ item: 2154 });                                                      // the palette: "grant <item> ..."
export { GrantPanel } from './GrantPanel';
export { openGrantDialog } from './GrantDialog';
export {
  createGrantStore,
  GrantProvider,
  type GrantInit,
  type GrantState,
  type GrantStore,
  type Recipient,
} from './store';
export { ensureGrantData, useGrantData } from './data';
export { ItemIcon } from './ItemIcon';
export { ItemPicker, type ItemPickerProps } from './ItemPicker';
export { Preview } from './ItemPreview';
