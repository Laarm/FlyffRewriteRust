// The views of the "guilds" category (Plan/25 section 2.2, 6; WP-ADMIN-24): the guild list, the drawer of a guild (roster, bank, bank log,
// wars, House, history) and the Create guild wizard with the logo picker and the cloak preview.
import { navigate } from '../../stores/route';
import type { CategoryModule } from '../../app/views';
import { CreateTab } from './CreateTab';
import { GuildDrawer } from './GuildDrawer';
import { GuildsTab } from './GuildsTab';
import './guilds.css';

const category: CategoryModule = {
  tabs: { guilds: GuildsTab, create: CreateTab },
  drawers: { guild: GuildDrawer },
  actions: [
    {
      id: 'guilds.create',
      label: 'Create guild',
      keywords: ['guild', 'logo', 'cloak', 'new', 'wizard'],
      cap: 'act.guild',
      icon: 'shield',
      run: () => {
        navigate({ category: 'guilds', tab: 'create', params: {} });
      },
    },
  ],
};

export default category;
