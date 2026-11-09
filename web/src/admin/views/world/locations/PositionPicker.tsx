// Picks a position on a static overview tile (Plan/25 section 7.4: "pick positions on a static tile preview"): the `admin-map` overview PNG of the world
// around the current position, a click sets x and z, the arrows move to the next tile and the select zooms out (a higher overview level covers more
// landscapes in the same 256 pixels). No map engine, no subscription: it is the picture the character drawer uses plus one click handler.
import { useEffect, useMemo, useState } from 'react';
import { Button, Select } from '../../../components';
import { landMeters, overviewUrl, TILE_PX } from '../../../components/mapGeo';
import { loadMeta } from '../../../components/mapMeta';
import type { MetaDoc } from '../../../world/metaTypes';
import { L } from './strings';
import '../../../components/staticMap.css';

export interface PositionPickerProps {
  readonly world: number;
  readonly x: number;
  readonly z: number;
  readonly onPick: (x: number, z: number) => void;
  /** Edge in CSS pixels (default 256). */
  readonly size?: number;
}

/** The world point at a click inside a tile image of `px` pixels (row 0 is the north edge). */
export function pointInTile(
  tx: number,
  tz: number,
  span: number,
  fx: number,
  fy: number,
): { x: number; z: number } {
  return { x: Math.round(tx * span + fx * span), z: Math.round((tz + 1) * span - fy * span) };
}

export function PositionPicker({ world, x, z, onPick, size = TILE_PX }: PositionPickerProps) {
  const [loaded, setLoaded] = useState<{ world: number; meta: MetaDoc | null } | null>(null);
  const [level, setLevel] = useState(1);
  const [offset, setOffset] = useState({ dx: 0, dz: 0 });
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
  const failed = loaded?.world === world && loaded.meta === null;

  const tiles = meta?.tiles ?? null;
  const gen = tiles?.gen ?? null;
  const maxLevel = tiles?.max_level ?? 0;
  const geometry = useMemo(() => {
    if (!meta || !tiles) return null;
    const lv = Math.max(1, Math.min(level, maxLevel));
    const span = landMeters(meta.geometry.mpu) * 2 ** lv;
    const grid = tiles.levels.find((l) => l.k === lv);
    const cols = grid?.cols ?? 1;
    const rows = grid?.rows ?? 1;
    const tx = Math.max(0, Math.min(cols - 1, Math.floor(x / span) + offset.dx));
    const tz = Math.max(0, Math.min(rows - 1, Math.floor(z / span) + offset.dz));
    return { lv, span, cols, rows, tx, tz };
  }, [meta, tiles, level, maxLevel, x, z, offset]);

  if (failed || (meta && (!tiles || !gen || maxLevel < 1))) {
    return <p className="adm-muted">{L.form.noTiles}</p>;
  }
  if (!meta || !geometry || !gen) return <div style={{ width: size, height: size }} />;
  const { lv, span, cols, rows, tx, tz } = geometry;
  const left = ((x - tx * span) / span) * 100;
  const top = (((tz + 1) * span - z) / span) * 100;
  const inside = left >= 0 && left <= 100 && top >= 0 && top <= 100;
  const move = (dx: number, dz: number): void => {
    setOffset((o) => ({ dx: o.dx + dx, dz: o.dz + dz }));
  };
  return (
    <div className="loc-picker" data-testid="position-picker">
      <button
        type="button"
        className="adm-staticmap"
        style={{ width: size, height: size }}
        aria-label={L.form.pick}
        title={L.form.pick}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          if (r.width <= 0 || r.height <= 0) return;
          const p = pointInTile(
            tx,
            tz,
            span,
            (e.clientX - r.left) / r.width,
            (e.clientY - r.top) / r.height,
          );
          onPick(p.x, p.z);
        }}
      >
        <img
          className="adm-staticmap__tile"
          src={overviewUrl(gen, meta.id, lv, tx, tz)}
          alt=""
          width={size}
          height={size}
          draggable={false}
        />
        {inside ? (
          <span
            className="adm-staticmap__dot"
            style={{ left: `${String(left)}%`, top: `${String(top)}%` }}
          />
        ) : null}
      </button>
      <div className="loc-picker__bar">
        <Select
          aria-label={L.form.level}
          value={lv}
          onChange={(e) => {
            setLevel(Number(e.target.value));
            setOffset({ dx: 0, dz: 0 });
          }}
        >
          {Array.from({ length: maxLevel }, (_, i) => i + 1).map((k) => (
            <option key={k} value={k}>
              {L.form.levelLabel(k)}
            </option>
          ))}
        </Select>
        <Button
          size="sm"
          variant="ghost"
          aria-label={L.form.previous}
          disabled={tx <= 0}
          onClick={() => {
            move(-1, 0);
          }}
        >
          ←
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label="North"
          disabled={tz >= rows - 1}
          onClick={() => {
            move(0, 1);
          }}
        >
          ↑
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label="South"
          disabled={tz <= 0}
          onClick={() => {
            move(0, -1);
          }}
        >
          ↓
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label={L.form.next}
          disabled={tx >= cols - 1}
          onClick={() => {
            move(1, 0);
          }}
        >
          →
        </Button>
      </div>
    </div>
  );
}
