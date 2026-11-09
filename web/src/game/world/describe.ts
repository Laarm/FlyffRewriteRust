// Describes a world for the sim, the renderer and the streamer from the client data (Plan/40 section 9.8 step 2): the `worlds`
// bundle (folder, title, `.wld` settings, continents), the asset manifest (`worlds[folder]`: lands, mpu, the lands that have a
// `.lnd`) and the world id (the housing mini-room). `null` while a piece is missing (the bundle or the manifest is not loaded);
// the loading gate then waits and asks again.

import type { ClientContext } from '../../app/context';
import type { WorldRenderDesc } from '../../render/types';
import { WI_WORLD_MINIROOM } from '../../gen/defs/world';
import type { StreamWorld } from '../../assets/land';
import type { SimWorldDesc } from '../ports';

export interface WorldDescription {
  readonly worldId: number;
  /** Lower-case folder name (the key of the asset manifest and of `/gen/collision`). */
  readonly folder: string;
  readonly title: string;
  readonly mpu: number;
  readonly landsW: number;
  readonly landsH: number;
  readonly sim: SimWorldDesc;
  readonly render: WorldRenderDesc;
  readonly stream: StreamWorld;
}

export function describeWorld(ctx: ClientContext, worldId: number): WorldDescription | null {
  const data = ctx.data;
  const row = data?.world(worldId);
  if (!data || !row) return null;
  const folderKey = row.folder.toLowerCase();
  const info = data.worldFolder(folderKey);
  const manifest = ctx.assetLayer.source.manifest;
  if (!info || !manifest) return null;
  const entry = manifest.worlds[folderKey];
  const settings = info.settings;
  const mpu = entry?.mpu ?? settings.mpu;
  const landsW = entry?.lands[0] ?? settings.size[0];
  const landsH = entry?.lands[1] ?? settings.size[1];
  return {
    worldId,
    folder: folderKey,
    title: row.title,
    mpu,
    landsW,
    landsH,
    sim: {
      worldId,
      mpu,
      landsW,
      landsH,
      flyAllowed: settings.fly,
      housing: worldId === WI_WORLD_MINIROOM,
    },
    render: {
      worldId,
      folder: folderKey,
      settings,
      continents: info.continents,
      mpu,
      landsW,
      landsH,
      indoor: settings.indoor,
      present: entry?.present ?? [],
    },
    stream: { worldId, folder: folderKey, mpu, landsW, landsH },
  };
}
