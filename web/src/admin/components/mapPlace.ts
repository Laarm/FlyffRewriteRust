// Where the map is, as a value: the world, the layer, a centre and a scale. Places travel as hash parameters (`#/world/map?w=1&l=0&x=6960&z=3274&zoom=2`,
// Plan/25 section 2.3, 3.2 "First view"), are remembered per browser, and are what the quick locations and the palette ask the map to open.
import type { MetaDoc, WorldsDoc } from '../world/metaTypes';

export interface Place {
  readonly world: number;
  readonly layer: number;
  /** The centre; absent = the default view of the world. */
  readonly x?: number;
  readonly z?: number;
  /** World units per CSS pixel; absent = keep the scale (or the default of the world). */
  readonly upp?: number;
}

/** The first view of a fresh browser: Flaris at 1:2 (about 1 km wide). */
export const DEFAULT_WORLD = 1;
export const DEFAULT_UPP = 2;
/**
 * The viewport width of the first view in metres. 1:2 is "about 1 km wide" on the plan's canvas; on a wider canvas the same scale shows more than
 * the server sends in detail: the subscription is the viewport plus 25 % on each side and tier A ends at 1,200 m, so the first view is at most 760 m
 * wide (a 1,140 m rectangle) and shows everything (items too) instead of the degraded overview.
 */
export const DEFAULT_VIEW_M = 760;

/** The scale of the first view for a canvas of `width` pixels: 1:2, finer when 1:2 would be wider than {@link DEFAULT_VIEW_M}. */
export function defaultUpp(width: number): number {
  if (!(width > 0)) return DEFAULT_UPP;
  return Math.min(DEFAULT_UPP, Math.max(0.25, DEFAULT_VIEW_M / width));
}

export const LAST_VIEW_KEY = 'flyff.admin.map.view.v1';

function num(v: string | undefined): number | undefined {
  if (v === undefined || v === '') return undefined;
  const n = Number(v);
  return Number.isFinite(n) ? n : undefined;
}

/** The place a route names; `null` when it has no world. */
export function placeFromParams(params: Readonly<Record<string, string>>): Place | null {
  const w = num(params['w']);
  if (w === undefined || !Number.isInteger(w) || w <= 0) return null;
  const l = num(params['l']);
  const x = num(params['x']);
  const z = num(params['z']);
  const zoom = num(params['zoom']);
  return {
    world: w,
    layer: l !== undefined && Number.isInteger(l) ? l : 0,
    ...(x !== undefined && z !== undefined ? { x, z } : {}),
    ...(zoom !== undefined && zoom > 0 ? { upp: zoom } : {}),
  };
}

/** The hash parameters of a place that may lack a centre or a scale (a jump from a list: only what the place has). */
export function placeParams(p: Place): Record<string, string> {
  const out: Record<string, string> = { w: String(p.world), l: String(p.layer) };
  if (p.x !== undefined && p.z !== undefined) {
    out['x'] = String(Math.round(p.x));
    out['z'] = String(Math.round(p.z));
  }
  if (p.upp !== undefined) out['zoom'] = String(Math.round(p.upp * 100) / 100);
  return out;
}

const round = (n: number, digits: number): number => {
  const f = 10 ** digits;
  return Math.round(n * f) / f;
};

/** The hash parameters of a place (the map's own keys; the caller keeps the others). */
export function paramsOfPlace(p: Required<Place>): Record<string, string> {
  return {
    w: String(p.world),
    l: String(p.layer),
    x: String(round(p.x, 0)),
    z: String(round(p.z, 0)),
    zoom: String(round(p.upp, 2)),
  };
}

/** The keys the map owns in the hash. */
export const MAP_PARAM_KEYS: readonly string[] = ['w', 'l', 'x', 'z', 'zoom', 'sel'];

/** `sel=p:55,m:120`: the selected object ids with a letter for their kind (p player, m monster, n npc, e pet, i item, c ctrl). */
export function formatSel(
  items: readonly { readonly id: number; readonly kind: number }[],
): string {
  const letters = ['', 'p', 'm', 'n', 'e', 'i', 'c'];
  return items
    .slice(0, 20)
    .map((i) => `${letters[i.kind] ?? 'c'}:${String(i.id)}`)
    .join(',');
}

/** The object ids of a `sel` parameter (letters are ignored: the ids are what the store knows). */
export function parseSel(text: string | undefined): number[] {
  if (text === undefined || text === '') return [];
  const out: number[] = [];
  for (const part of text.split(',')) {
    const id = Number(part.includes(':') ? part.slice(part.indexOf(':') + 1) : part);
    if (Number.isInteger(id) && id > 0 && !out.includes(id)) out.push(id);
  }
  return out.slice(0, 20);
}

/** What `localStorage` keeps of the last view. */
export function parseStoredPlace(raw: string | null): Place | null {
  if (raw === null) return null;
  try {
    const v = JSON.parse(raw) as Partial<Record<'world' | 'layer' | 'x' | 'z' | 'upp', unknown>>;
    if (typeof v.world !== 'number' || !Number.isInteger(v.world) || v.world <= 0) return null;
    const layer = typeof v.layer === 'number' && Number.isInteger(v.layer) ? v.layer : 0;
    const ok = (n: unknown): n is number => typeof n === 'number' && Number.isFinite(n);
    return {
      world: v.world,
      layer,
      ...(ok(v.x) && ok(v.z) ? { x: v.x, z: v.z } : {}),
      ...(ok(v.upp) && v.upp > 0 ? { upp: v.upp } : {}),
    };
  } catch {
    return null;
  }
}

/** Flaris from the data: the `teleport16` row named "Flaris" of the world, else the `.wld` camera, else the middle of the world. */
export function defaultCentre(meta: MetaDoc): { x: number; z: number } {
  const tp = meta.teleports.find((t) => /flaris/i.test(t.name)) ?? meta.teleports[0];
  if (tp) return { x: tp.pos[0], z: tp.pos[2] };
  if (meta.camera) return { x: meta.camera.x, z: meta.camera.z };
  return { x: meta.geometry.extent_m[0] / 2, z: meta.geometry.extent_m[1] / 2 };
}

/** The world to open when the wanted one is not loaded: the first world that has a minimap. */
export function fallbackWorld(worlds: WorldsDoc, wanted: number): number {
  if (worlds.worlds.some((w) => w.id === wanted)) return wanted;
  return worlds.worlds.find((w) => w.tiles !== null)?.id ?? worlds.worlds[0]?.id ?? wanted;
}

/** The scale that shows a whole world's width in a canvas of `w` pixels (dungeons: "fit world"). */
export function scaleForWorld(meta: MetaDoc, w: number, h: number): number {
  const e = meta.geometry.extent_m;
  return Math.max((e[0] * 1.05) / Math.max(1, w), (e[1] * 1.05) / Math.max(1, h));
}
