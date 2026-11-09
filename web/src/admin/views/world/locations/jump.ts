// Jumping to a quick location on the open map (Plan/25 section 3.13): the layer follows the policy of the row, the scale its zoom preset.
// `fixed0` opens layer 0; `pick_live` opens the only live layer, layer 0 (the map then says "empty template layer") when there is none, and asks with
// the layer popover when there are several. The map keeps its place and flies (350 ms), no history entry is made (`views/world/jump.ts` is for
// opening the map from other tabs).
import { createElement } from 'react';
import { defaultUpp, type Place } from '../../../components/mapPlace';
import { openDialog } from '../../../stores/dialogs';
import type { MapApi } from '../../../world/api';
import { LayerPopover } from '../LayerPopover';
import { layersOfWorld, useLayers } from '../layers';
import type { LocationRow } from './api';
import { useLocations } from './store';

/**
 * The scale of a jump in world units per pixel. A bookmark's own zoom is the operator's choice and is kept; a preset of a group is capped so that the
 * view stays inside the detailed tier of the map (a 1:2 town on a wide canvas would be 2 km across). `undefined` fits the world or keeps the scale.
 */
export function zoomOf(row: LocationRow, canvasWidth: number): number | undefined {
  if (row.own_zoom !== null) return row.own_zoom;
  if (row.zoom === null) return undefined;
  return Math.min(row.zoom, defaultUpp(canvasWidth));
}

/** The place of a row on layer `layer`. */
export function placeOf(row: LocationRow, layer: number, canvasWidth: number): Place {
  const upp = zoomOf(row, canvasWidth);
  return {
    world: row.world,
    layer,
    x: row.x,
    z: row.z,
    ...(upp === undefined ? {} : { upp }),
  };
}

/** Flies to a location; resolves when the map arrived (or the operator closed the layer popover without a choice). */
export async function jumpToLocation(api: MapApi, row: LocationRow): Promise<void> {
  const width = api.store.getState().view.w;
  const go = async (layer: number): Promise<void> => {
    useLocations.getState().recordJump(row.id);
    await api.flyTo(placeOf(row, layer, width));
  };
  if (row.layer_policy === 'fixed0') {
    await go(0);
    return;
  }
  const live = layersOfWorld(useLayers.getState().layers, row.world);
  const only = live[0];
  if (live.length === 0 || !only) {
    await go(0);
    return;
  }
  if (live.length === 1) {
    await go(only.layer);
    return;
  }
  await new Promise<void>((resolve) => {
    openDialog((close) =>
      createElement(LayerPopover, {
        world: row.world,
        onChoose: (layer: number) => {
          close();
          void go(layer).then(resolve);
        },
        onClose: () => {
          close();
          resolve();
        },
      }),
    );
  });
}
