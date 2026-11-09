// A static preview of a position (Plan/25 section 3.8): a plain `<img>` of the level-1 `admin-map` overview tile that contains the point, with a CSS dot
// on it. It loads no map code, no WASM and no subscription (the engine is a lazy chunk of the Map tab); one small PNG and, for the tile URL, the world's
// meta document (cached). A click opens the Map tab at the position. The character drawer's position card and the locations editor use it.
import { useEffect, useState } from 'react';
import { navigate } from '../stores/route';
import { loadMeta } from './mapMeta';
import { landMeters, overviewPixel, overviewUrl, TILE_PX } from './mapGeo';
import { placeParams } from './mapPlace';
import type { MetaDoc } from '../world/metaTypes';
import './staticMap.css';

export interface StaticMapPreviewProps {
  readonly world: number;
  readonly x: number;
  readonly z: number;
  /** The layer the position is in (opens that layer on the map). */
  readonly layer?: number;
  /** Edge in CSS pixels (default 160). */
  readonly size?: number;
  /** Accessible name; default "Open on the map". */
  readonly label?: string;
  /** Replaces the default click (navigate to the Map tab at this position). */
  readonly onOpen?: () => void;
}

/** The overview level of the previews: one tile covers 2 x 2 landscapes (about 1 km at MPU 4). */
const PREVIEW_LEVEL = 1;

/** Where the dot sits inside the tile image, in percent of its edge, and the tile that contains the point. */
export function previewDot(
  x: number,
  z: number,
  mpu: number,
): { tx: number; tz: number; left: number; top: number } {
  const p = overviewPixel(x, z, landMeters(mpu), PREVIEW_LEVEL);
  return { tx: p.tx, tz: p.tz, left: (p.u / TILE_PX) * 100, top: (p.v / TILE_PX) * 100 };
}

export function StaticMapPreview({
  world,
  x,
  z,
  layer = 0,
  size = 160,
  label = 'Open on the map',
  onOpen,
}: StaticMapPreviewProps) {
  const [loaded, setLoaded] = useState<{ world: number; meta: MetaDoc | null } | null>(null);
  const [brokenFor, setBrokenFor] = useState<number | null>(null);
  useEffect(() => {
    let live = true;
    loadMeta(world).then(
      (m) => {
        if (live) setLoaded({ world, meta: m });
      },
      () => {
        if (live) setLoaded({ world, meta: null });
      },
    );
    return () => {
      live = false;
    };
  }, [world]);

  const meta = loaded?.world === world ? loaded.meta : null;
  const tiles = meta?.tiles ?? null;
  const gen = tiles?.gen ?? null;
  const hasImage =
    meta !== null &&
    tiles !== null &&
    gen !== null &&
    tiles.max_level >= PREVIEW_LEVEL &&
    brokenFor !== world;
  const dot = meta ? previewDot(x, z, meta.geometry.mpu) : { tx: 0, tz: 0, left: 50, top: 50 };
  const open = (): void => {
    if (onOpen) onOpen();
    else
      navigate({
        category: 'world',
        tab: 'map',
        params: placeParams({ world, layer, x, z, upp: 1 }),
      });
  };
  return (
    <button
      type="button"
      className="adm-staticmap"
      style={{ width: size, height: size }}
      aria-label={`${label}: ${String(Math.round(x))}, ${String(Math.round(z))}`}
      title={label}
      onClick={open}
    >
      {hasImage ? (
        <img
          className="adm-staticmap__tile"
          src={overviewUrl(gen, meta.id, PREVIEW_LEVEL, dot.tx, dot.tz)}
          alt=""
          width={size}
          height={size}
          draggable={false}
          onError={() => {
            setBrokenFor(world);
          }}
        />
      ) : null}
      <span
        className="adm-staticmap__dot"
        style={{ left: `${String(dot.left)}%`, top: `${String(dot.top)}%` }}
      />
    </button>
  );
}
