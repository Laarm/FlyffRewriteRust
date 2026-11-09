// The slots of the live map that other work packages fill (Plan/25 section 3.2): the map (`world/MapView.tsx`) renders a built-in default for each
// empty slot, so it works as it is. The inspector, the tools, the locations panel and the overlay (hover card, context menu, the "You" chip) are the
// components of WP-ADMIN-13 and WP-ADMIN-14; each loads as a chunk of its own when the map first shows it, so the Instances tab and the other
// categories never pay for them.
//
// A slot component gets `{ api }` (`world/api.ts`: selection, entities, follow, `flyTo`, `pickPoint`, highlights, context-menu events) and reads the
// map's store with `useStore(api.store, selector)`. Import the contract with `import type` only.
import { Suspense, lazy, type ComponentType, type ReactElement } from 'react';
import { Skeleton } from '../../components/Spinner';
import type { MapSlotProps, MapSlots } from '../../world/api';

type LocationsProps = MapSlotProps & { readonly rail: boolean; readonly onToggleRail: () => void };

const Inspector = lazy(() => import('./Inspector'));
const Tools = lazy(() => import('./Tools'));
const Overlay = lazy(() => import('./ContextMenuHost'));
const Locations = lazy(() => import('./locations/LocationsPanel'));

/** A lazily loaded component with its own boundary: the map around it keeps drawing while the chunk loads. */
function withBoundary<P extends object>(
  Lazy: ComponentType<P>,
  fallback: ReactElement | null,
): ComponentType<P> {
  function Bounded(props: P) {
    return (
      <Suspense fallback={fallback}>
        <Lazy {...props} />
      </Suspense>
    );
  }
  return Bounded;
}

export const mapSlots: MapSlots = {
  inspector: withBoundary<MapSlotProps>(Inspector, <Skeleton lines={4} />),
  tools: withBoundary<MapSlotProps>(Tools, <Skeleton lines={3} />),
  overlay: withBoundary<MapSlotProps>(Overlay, null),
  locations: withBoundary<LocationsProps>(Locations, <Skeleton lines={5} />),
};
