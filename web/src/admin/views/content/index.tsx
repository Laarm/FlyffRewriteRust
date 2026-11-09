// The views of the "content" category (Plan/25 section 2.2, 7; WP-ADMIN-30): Rates & events (with the Event.lua modifiers and the weather),
// Spawns (the respawn lines with "Respawn now" and the monster census) and Content (the registry of the panels of the later systems, 7.3).
// The Locations tab is the editor of the shared map bookmarks (WP-ADMIN-14, `views/world/locations/`).
import type { CategoryModule } from '../../app/views';
import './content.css';
import { ContentTab } from './ContentTab';
import { RatesTab } from './RatesTab';
import { SpawnsTab } from './SpawnsTab';
import { LocationsTab } from '../world/locations/LocationsTab';

const category: CategoryModule = {
  tabs: {
    rates: RatesTab,
    spawns: SpawnsTab,
    content: ContentTab,
    locations: LocationsTab,
  },
};

export default category;
