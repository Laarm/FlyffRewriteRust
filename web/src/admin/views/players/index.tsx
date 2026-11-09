// The views of the "players" category (Plan/25 section 2.2, 4; WP-ADMIN-18, WP-ADMIN-19): the Online table, the Characters search,
// the Accounts list, Moderation (bans, mutes, login blocks, IP blocks, notes, alts, chat), and the drawers of a character (with live
// inventories and banks) and of an account. Staff & access (`server/staff`) lives in `views/server/StaffAccess.tsx`.
import { navigate } from '../../stores/route';
import type { CategoryModule } from '../../app/views';
import { AccountDrawer } from './AccountDrawer';
import { AccountsTab } from './AccountsTab';
import { CharacterDrawer } from './CharacterDrawer';
import { CharactersTab } from './CharactersTab';
import { ModerationTab } from './ModerationTab';
import { OnlineTab } from './OnlineTab';
import './players.css';

const category: CategoryModule = {
  tabs: {
    online: OnlineTab,
    characters: CharactersTab,
    accounts: AccountsTab,
    moderation: ModerationTab,
  },
  drawers: {
    character: CharacterDrawer,
    account: AccountDrawer,
  },
  actions: [
    {
      id: 'players.find',
      label: 'Find a character...',
      keywords: ['player', 'search', 'profile', 'inventory', 'bank'],
      cap: 'view.players',
      icon: 'users',
      run: () => {
        navigate({ category: 'players', tab: 'characters', params: {} });
      },
    },
    {
      id: 'players.chat',
      label: 'Open the chat monitor',
      keywords: ['chat', 'say', 'shout', 'whisper', 'moderation'],
      cap: 'view.chat',
      icon: 'message',
      run: () => {
        navigate({ category: 'players', tab: 'moderation', params: { ms: 'chat' } });
      },
    },
    {
      id: 'players.bans',
      label: 'Show the active bans',
      keywords: ['ban', 'unban', 'moderation'],
      cap: 'view.players',
      icon: 'shield',
      run: () => {
        navigate({ category: 'players', tab: 'moderation', params: {} });
      },
    },
  ],
};

export default category;
