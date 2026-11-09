// The Map tab: a thin wrapper that loads the map engine on demand (Plan/25 section 3.8: "The Map tab is a lazy chunk"). Nothing of the map, no
// tile and no subscription exists before this tab is selected, and the engine chunk is not fetched for the other tabs of this category.
import { Suspense, lazy } from 'react';
import type { TabProps } from '../../app/views';
import { Skeleton } from '../../components/Spinner';
import { mapSlots } from './slots';

const MapView = lazy(() => import('../../world/MapView'));

export function MapTab({ route }: TabProps) {
  return (
    <Suspense fallback={<Skeleton lines={6} height={18} />}>
      <MapView route={route} slots={mapSlots} />
    </Suspense>
  );
}
