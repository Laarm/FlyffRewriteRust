// The actions of a location button (Plan/25 section 3.13 "Behaviour"): open it, teleport the selected players there, go there with the acting
// character, copy `/te`, edit it (a shared bookmark, `act.world`; a default button is edited as a copy that replaces it), delete the bookmark and
// the favourite toggle.
import { runConfirmed } from '../../../app/mutate';
import type { MapApi } from '../../../world/api';
import {
  goToMapPoint,
  selectedPlayers,
  teleportPlayers,
  copyTe,
  type ActionEnv,
  type MapAction,
} from '../InspectorActions';
import { layersOfWorld, useLayers } from '../layers';
import { deleteBookmark, type LocationRow } from './api';
import { openEditLocation } from './BookmarkDialog';
import { jumpToLocation } from './jump';
import { L } from './strings';
import { MAX_FAVOURITES, useLocations } from './store';

/** The layer a teleport to the row uses: layer 0, or the only live layer of a layered world. */
export function layerOfRow(row: LocationRow): number {
  if (row.layer_policy === 'fixed0') return 0;
  const live = layersOfWorld(useLayers.getState().layers, row.world);
  return live.length === 1 ? (live[0]?.layer ?? 0) : 0;
}

export function removeBookmark(row: LocationRow): Promise<boolean> {
  const id = row.bookmark_id;
  if (id === null) return Promise.resolve(false);
  return runConfirmed({
    title: L.editor.deleteTitle(row.name),
    message: L.editor.deleteMessage,
    confirmLabel: L.editor.delete,
    danger: true,
    run: async (given) => {
      await deleteBookmark(id, given.reason);
      await useLocations.getState().load(true);
    },
    success: L.editor.deleted(row.name),
  }).then((o) => o.ok);
}

export function locationActions(env: ActionEnv, api: MapApi, row: LocationRow): MapAction[] {
  const has = (cap: string): boolean => env.caps.has(cap);
  const st = useLocations.getState();
  const favourite = st.favourites.includes(row.id);
  const players = selectedPlayers(api);
  const at = { world: row.world, layer: layerOfRow(row), x: row.x, z: row.z };
  const out: MapAction[] = [
    {
      id: 'open',
      label: L.menu.open,
      run: () => jumpToLocation(api, row),
    },
  ];
  if (has('act.teleport.others')) {
    out.push({
      id: 'teleport-here',
      label: players.length > 1 ? L.menu.teleportMany(players.length) : L.menu.teleportHere,
      ...(players.length === 0 ? { disabled: L.menu.noPlayer } : {}),
      run: () => teleportPlayers(env, players, at, { label: 'location' }),
    });
  }
  if (has('act.teleport.self')) {
    out.push({
      id: 'go-here',
      label: L.menu.goHere,
      ...(env.acting === null ? { disabled: 'Needs an acting GM character.' } : {}),
      run: () => goToMapPoint(env, at),
    });
  }
  out.push({
    id: 'copy-te',
    label: L.menu.copyTe,
    run: () => {
      copyTe(row.world, row.x, row.z);
    },
  });
  if (has('act.world')) {
    out.push({
      id: 'edit',
      label: row.kind === 'bookmark' ? L.menu.edit : L.menu.override,
      separator: true,
      run: () => {
        openEditLocation(row);
      },
    });
    if (row.kind === 'bookmark') {
      out.push({
        id: 'delete',
        label: L.menu.delete,
        danger: true,
        run: () => removeBookmark(row),
      });
    }
  }
  out.push({
    id: 'favourite',
    label: favourite ? L.menu.removeFavourite : L.menu.addFavourite,
    separator: !has('act.world'),
    ...(!favourite && st.favourites.length >= MAX_FAVOURITES
      ? { disabled: L.menu.favouritesFull }
      : {}),
    run: () => {
      st.toggleFavourite(row.id);
    },
  });
  return out;
}
