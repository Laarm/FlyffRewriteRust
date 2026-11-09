// The atlas layer of the map (Plan/25 section 3.4, MAY; Madrigal only): the eight world-map artworks of the original client (`Theme/WORLD_*.dds`)
// drawn into the rectangles of the `worlds` bundle's `world_maps` (e.g. Flaris `[5500, 2540, 9200, 5130]`), the top edge at the higher z. It is
// decorative and off by default: the rectangles are the original world-map window's and only approximately aligned with the minimap tiles.
// The artworks have PNG forms (`/gen/png`), loaded when the layer is first switched on.
import { assetUrl } from '../../assets/paths';
import { bitmapOf, bundleUrl, getJson, loadResolver } from './gameAssets';
import type { AtlasImage } from './overlays';

/** The rows of `world_maps` that carry what the layer needs. */
export interface WorldMapRect {
  readonly map: number;
  readonly texture: string;
  /** `[x1, z1, x2, z2]` in world coordinates. */
  readonly rect: readonly [number, number, number, number];
}

/** The `world_maps` of a `worlds` bundle envelope (anything malformed is left out). */
export function worldMapRects(envelope: unknown): WorldMapRect[] {
  const rows = (envelope as { data?: { world_maps?: unknown } } | null)?.data?.world_maps;
  if (!Array.isArray(rows)) return [];
  const out: WorldMapRect[] = [];
  for (const row of rows as unknown[]) {
    if (typeof row !== 'object' || row === null) continue;
    const r = row as { map?: unknown; texture?: unknown; rect?: unknown };
    if (typeof r.map !== 'number' || typeof r.texture !== 'string') continue;
    if (
      !Array.isArray(r.rect) ||
      r.rect.length !== 4 ||
      !r.rect.every((n) => typeof n === 'number')
    )
      continue;
    const rect = r.rect as [number, number, number, number];
    out.push({ map: r.map, texture: r.texture, rect });
  }
  return out.sort((a, b) => a.map - b.map);
}

/** Loads the artworks (the whole-Madrigal map first, so that the continents lie over it). Resolves with what could be loaded. */
export async function loadAtlas(
  f: (url: string) => Promise<Response> = (u) => fetch(u, { credentials: 'same-origin' }),
): Promise<AtlasImage[]> {
  const [manifest, resolved] = await Promise.all([
    getJson(f, '/data/manifest.json').catch(() => null),
    loadResolver(f),
  ]);
  const url = manifest === null ? null : bundleUrl(manifest, 'worlds');
  if (url === null || resolved === null) return [];
  let rows: WorldMapRect[];
  try {
    rows = worldMapRects(await getJson(f, url));
  } catch {
    return [];
  }
  const loaded = await Promise.all(
    rows.map(async (r): Promise<AtlasImage | null> => {
      const u = assetUrl(resolved, r.texture);
      const image = u === null ? undefined : await bitmapOf(f, u);
      return image ? { image, rect: r.rect } : null;
    }),
  );
  return loaded.filter((x): x is AtlasImage => x !== null);
}
