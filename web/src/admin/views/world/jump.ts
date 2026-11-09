// Opening the live map at a place from anywhere in the console (the locations panel, the instances tab, the static previews, the palette): the
// map's place is part of its hash (`#/world/map?w=&l=&x=&z=&zoom=`), so a jump is a navigation. A location whose world is `pick_live` asks which of
// the live layers to open (`LayerPopover`): one live layer opens straight away, none opens layer 0 (the empty template layer).
import { placeParams, type Place } from '../../components/mapPlace';
import { navigate } from '../../stores/route';
import { openDialog } from '../../stores/dialogs';
import type { LayerPolicy } from '../../world/metaTypes';
import { LayerPopover } from './LayerPopover';
import { layersOfWorld, useLayers } from './layers';
import { createElement } from 'react';

/** Opens the Map tab at `place` (a new history entry: Back returns to where the user was). */
export function openOnMap(place: Place): void {
  navigate({ category: 'world', tab: 'map', params: placeParams(place) });
}

/**
 * A click on a location (Plan/25 section 3.13): `fixed0` opens layer 0; `pick_live` looks at the live layers of the world: exactly one opens
 * there, none opens layer 0, several ask with the popover (owner, members, age; most members first).
 */
export function openLocation(place: Omit<Place, 'layer'>, policy: LayerPolicy): void {
  if (policy === 'fixed0') {
    openOnMap({ ...place, layer: 0 });
    return;
  }
  const live = layersOfWorld(useLayers.getState().layers, place.world);
  if (live.length === 0) {
    openOnMap({ ...place, layer: 0 });
    return;
  }
  const only = live[0];
  if (live.length === 1 && only) {
    openOnMap({ ...place, layer: only.layer });
    return;
  }
  openDialog((close) =>
    createElement(LayerPopover, {
      world: place.world,
      onChoose: (layer: number) => {
        close();
        openOnMap({ ...place, layer });
      },
      onClose: close,
    }),
  );
}
