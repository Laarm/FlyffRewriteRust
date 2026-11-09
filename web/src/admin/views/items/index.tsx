// The views of the "items" category (Plan/25 section 2.2; WP-ADMIN-21, WP-ADMIN-22): the Item builder (the grant UI at full width), Kits &
// templates, Grants & removals, Item search and the Economy overview; the item drawer; the palette entry that opens the grant dialog.
import type { CategoryModule } from '../../app/views';
import { openGrantDialog } from '../../grant';
import { BuilderTab } from './BuilderTab';
import { EconomyTab } from './EconomyTab';
import { GrantsTab } from './GrantsTab';
import { ItemDrawer } from './ItemDrawer';
import { KitsTab } from './KitsTab';
import { SearchTab } from './SearchTab';
import { T } from './strings';

const category: CategoryModule = {
  tabs: {
    builder: BuilderTab,
    kits: KitsTab,
    grants: GrantsTab,
    search: SearchTab,
    economy: EconomyTab,
  },
  drawers: { item: ItemDrawer },
  actions: [
    {
      id: 'items.grant',
      label: T.actions.grant,
      keywords: ['give', 'item', 'penya', 'gold', 'kit'],
      cap: 'act.items',
      icon: 'package',
      run: () => {
        openGrantDialog();
      },
    },
  ],
};

export default category;
