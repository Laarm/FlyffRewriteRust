// The views of the "world" category (Plan/25 section 2.2, 3): the Live World map and the Instances & layers tab. The map itself is a lazy chunk of
// its own (`world/MapView`, loaded by `MapTab` when the Map tab is selected), so this module stays small and the other tabs never fetch it.
// The inspector, tools and locations of the map are filled through `slots.tsx`.
import type { CategoryModule } from '../../app/views';
import { Instances } from './Instances';
import { MapTab } from './MapTab';

const category: CategoryModule = {
  tabs: { map: MapTab, instances: Instances },
};

export default category;
