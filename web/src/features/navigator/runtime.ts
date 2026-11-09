// What the navigator reads of the running client (WP-CFEAT-28): the movers in view with the facts the dot rules need, the position of the
// player and the heading of the camera, the world. Thin adapters over the sim render view, the entity table and the stores - no rules of
// their own (those are `dots.ts`, `geometry.ts`, `scene.ts`).

import type { ClientContext } from '../../app/context';
import { EntityKind } from '../../game/entities';
import type { RenderViewReader } from '../../game/renderView';
import { RENDER_VIEW } from '../../gen/sim/wire';
import type { InputInstance } from '../../input/input';
import { type ContinentPoly, continentPolys } from './continent';
import type { MoverSample } from './dots';
import { arrowHeading } from './geometry';

const MV = RENDER_VIEW.mover;
const STRIDE = RENDER_VIEW.MOVER_STRIDE;
/** `TRANSPARENT_MODE` of the mover mode word. */
export const MODE_TRANSPARENT = 0x00000002;
/** `AUTH_GAMEMASTER` ('L'). */
export const AUTH_GAMEMASTER = 76;

/** The render view when it is valid. */
export function simView(ctx: ClientContext): RenderViewReader | null {
  const v = ctx.sim.view;
  return v?.valid === true ? v : null;
}

/** The world the navigator shows. */
export interface NavWorld {
  readonly worldId: number;
  readonly folder: string;
  readonly lands: readonly [number, number];
  readonly indoor: boolean;
}

export function worldOf(ctx: ClientContext): NavWorld | null {
  const w = ctx.stores.worldStore.getState();
  if (w.folder === '' || w.lands[0] <= 0 || w.lands[1] <= 0) return null;
  const settings = ctx.data?.worldFolder(w.folder)?.settings;
  return {
    worldId: w.worldId,
    folder: w.folder,
    lands: w.lands,
    indoor: settings?.indoor === true,
  };
}

/** `IsWorldInstanceDungeon()`: `WI_INSTANCE_OMINOUS <= id <= WI_INSTANCE_LAST_ID`. */
export function isInstanceWorld(worldId: number): boolean {
  return worldId >= 121 && worldId <= 125;
}

/** The player's position (LH): the render view, else the last known position of the self store. */
export function selfPosition(ctx: ClientContext): { x: number; y: number; z: number } | null {
  const rec = ctx.entities.self();
  if (!rec) return null;
  const out: [number, number, number] = [0, 0, 0];
  const v = simView(ctx);
  if (v?.posOf(rec.objId, out) === true) return { x: out[0], y: out[1], z: out[2] };
  const p = ctx.stores.selfStore.getState().pos;
  return { x: p[0], y: p[1], z: p[2] };
}

/** The heading of the camera in degrees clockwise from north (`g_Neuz.m_camera.m_vPos` -> the player). */
export function cameraHeading(ctx: ClientContext, px: number, pz: number): number {
  const core = (ctx.input as Partial<InputInstance>).core;
  const eye = core?.displayed?.pos;
  if (!eye) return 0;
  return arrowHeading(eye[0], eye[2], px, pz);
}

/** The quest mark of an NPC (`m_nQuestEmoticonIndex`): -1 for none. The quest feature installs it (`setQuestEmoticonSource`); the shipped, quest-less data never does. */
export type QuestEmoticonSource = (objId: number) => number;

/** The facts of one mover record. */
export function sampleOf(
  ctx: ClientContext,
  id: number,
  x: number,
  z: number,
  emoticon?: QuestEmoticonSource | null,
): MoverSample | null {
  const rec = ctx.entities.get(id);
  if (!rec || rec.removed) return null;
  if (rec.kind !== EntityKind.Player && rec.kind !== EntityKind.Npc) return null;
  const vis = rec.visual;
  const isPlayer = rec.kind === EntityKind.Player;
  const prop = isPlayer ? undefined : ctx.data?.mover(rec.propId);
  const structure = !isPlayer && vis?.structure != null ? Number(vis.structure) : -1;
  return {
    id,
    idPlayer: isPlayer ? (rec.idPlayer ?? 0) : 0,
    isPlayer,
    peaceful: vis?.peaceful ?? false,
    transparent: ((vis?.mode ?? 0) & MODE_TRANSPARENT) !== 0,
    gm: (vis?.authorization ?? 0) >= AUTH_GAMEMASTER,
    guildId: vis?.guild?.idGuild ?? 0,
    rank: prop?.class ?? -1,
    structure: Number.isFinite(structure) ? structure : -1,
    questEmoticon: isPlayer || !emoticon ? -1 : emoticon(id),
    x,
    z,
    name: vis?.name ?? '',
  };
}

/** Every mover in view (players and NPCs / monsters) with its position. */
export function readMovers(
  ctx: ClientContext,
  out: MoverSample[] = [],
  emoticon?: QuestEmoticonSource | null,
): MoverSample[] {
  out.length = 0;
  const v = simView(ctx);
  if (!v) return out;
  const f = v.movers;
  v.forEachMover((index, id) => {
    const at = index * STRIDE + MV.pos;
    const s = sampleOf(ctx, id, f[at] ?? 0, f[at + 2] ?? 0, emoticon);
    if (s) out.push(s);
  });
  return out;
}

/** The continent polygons of Madrigal (computed once per bundle arrival). */
export function madrigalPolys(ctx: ClientContext): readonly ContinentPoly[] {
  const defs = ctx.data?.worldFolder('wdmadrigal')?.continents;
  return defs ? continentPolys(defs) : [];
}

/** The position of an object (player, NPC, item, ctrl) from the render view; `null` when unknown. */
export function positionOf(ctx: ClientContext, id: number): { x: number; z: number } | null {
  const v = simView(ctx);
  if (!v) return null;
  const out: [number, number, number] = [0, 0, 0];
  return v.posOf(id, out) ? { x: out[0], z: out[2] } : null;
}
