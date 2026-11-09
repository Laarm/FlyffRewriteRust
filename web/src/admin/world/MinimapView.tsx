// The minimap inset (Plan/25 section 3.2): a 180 px canvas with the overview tile of the world, the viewport rectangle (drag it to move the map) and
// one dot per online player of the world, from the roster stream. It opens NO map subscription and fetches one overview PNG.
import { useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { overviewUrl } from '../components/mapGeo';
import { safeStorage } from '../../platform/storage';
import {
  drawMinimap,
  fromMinimap,
  minimapGeometry,
  overviewSource,
  type MinimapDot,
} from './minimap';
import type { MapSession } from './session';
import { M } from './strings';

const KEY = 'flyff.admin.map.minimap.v1';

export function MinimapView({ session }: { readonly session: MapSession }) {
  const meta = useStore(session.store, (s) => s.meta);
  const view = useStore(session.store, (s) => s.view);
  const follow = useStore(session.store, (s) => s.follow);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [open, setOpen] = useState(() => safeStorage.local.get(KEY) !== 'hidden');
  const [loaded, setLoaded] = useState<{ world: number; bitmap: ImageBitmap } | null>(null);
  const [version, setVersion] = useState(0);
  const image = loaded !== null && loaded.world === meta?.id ? loaded.bitmap : null;

  const extent = meta?.geometry.extent_m;
  const geo = extent ? minimapGeometry(extent) : null;

  // the overview tile of the whole world: the top level is ONE tile
  useEffect(() => {
    const tiles = meta?.tiles;
    if (!meta || !tiles?.gen || tiles.max_level < 1) return;
    const ctl = new AbortController();
    void (async () => {
      try {
        const r = await fetch(overviewUrl(tiles.gen ?? '', meta.id, tiles.max_level, 0, 0), {
          signal: ctl.signal,
          credentials: 'same-origin',
        });
        if (!r.ok) return;
        const bmp = await createImageBitmap(await r.blob());
        if (ctl.signal.aborted) bmp.close();
        else setLoaded({ world: meta.id, bitmap: bmp });
      } catch {
        // no overview: the void and the dots stand
      }
    })();
    return () => {
      ctl.abort();
    };
  }, [meta]);

  useEffect(() => {
    return () => {
      image?.close();
    };
  }, [image]);

  // the roster changes every two seconds
  useEffect(() => {
    return session.roster.onChange(() => {
      setVersion((v) => v + 1);
    });
  }, [session]);

  useEffect(() => {
    const c = canvas.current;
    if (!c || !geo || !meta || !open) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    c.width = Math.round(geo.w * dpr);
    c.height = Math.round(geo.h * dpr);
    const ctx = c.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const cs = getComputedStyle(document.documentElement);
    const dots: MinimapDot[] = session.roster
      .inWorld(meta.id)
      .map((r) => ({ x: r.x, z: r.z, world: r.world }));
    const row =
      follow && follow.characterId !== 0 ? session.roster.rows.get(follow.characterId) : undefined;
    const tiles = meta.tiles;
    drawMinimap(
      ctx,
      geo,
      image,
      tiles && tiles.max_level >= 1
        ? overviewSource(meta.geometry.extent_m, meta.geometry.mpu, tiles.max_level)
        : null,
      dots,
      view,
      {
        void: cs.getPropertyValue('--map-void').trim() || '#cfd5df',
        dot: '#ffffff',
        edge: '#0b1220',
        box: cs.getPropertyValue('--accent').trim() || '#2458c5',
        follow: '#33e0ff',
      },
      row ? { x: row.x, z: row.z } : null,
    );
  }, [geo, meta, image, view, version, open, follow, session]);

  const move = (e: React.PointerEvent<HTMLCanvasElement>): void => {
    if (!geo) return;
    const r = e.currentTarget.getBoundingClientRect();
    const p = fromMinimap(geo, e.clientX - r.left, e.clientY - r.top);
    session.dragged();
    session.centreOn(p.x, p.z);
  };

  if (!geo || !meta) return null;
  if (!open) {
    return (
      <button
        type="button"
        className="map-minimap-toggle"
        aria-label={M.minimapShow}
        title={M.minimapShow}
        onClick={() => {
          safeStorage.local.set(KEY, 'shown');
          setOpen(true);
        }}
      >
        {M.minimap}
      </button>
    );
  }
  return (
    <div className="map-minimap" data-testid="minimap">
      <button
        type="button"
        className="map-minimap__hide"
        aria-label={M.minimapHide}
        title={M.minimapHide}
        onClick={() => {
          safeStorage.local.set(KEY, 'hidden');
          setOpen(false);
        }}
      >
        ×
      </button>
      <canvas
        ref={canvas}
        className="map-minimap__canvas"
        style={{ width: geo.w, height: geo.h }}
        aria-label={M.minimap}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          move(e);
        }}
        onPointerMove={(e) => {
          if (e.buttons === 1) move(e);
        }}
      />
    </div>
  );
}
