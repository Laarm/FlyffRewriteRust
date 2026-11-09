// `GET /admin/api/guilds/meta` as React state, fetched once per page (the limits and the caps do not change while the console is open) and
// `useGuildIndex()`: the id -> {name, logo, level} of every guild, one `GET /admin/api/guilds?limit=500` cached for 30 s, so the character
// drawer, the Online table and the map inspector can show a logo and a name instead of `Guild #id`.
import { useEffect, useState } from 'react';
import { fetchPage } from '../../api/rest';
import type { GuildMeta, GuildRow } from '../../gen/dto';
import { fetchGuildMeta } from './api';

let metaPromise: Promise<GuildMeta> | null = null;

/** Tests: forgets the cached metadata and index. */
export function resetGuildCachesForTests(): void {
  metaPromise = null;
  index = null;
}

export function loadGuildMeta(): Promise<GuildMeta> {
  metaPromise ??= fetchGuildMeta().catch((e: unknown) => {
    metaPromise = null;
    throw e;
  });
  return metaPromise;
}

export interface MetaState {
  readonly meta: GuildMeta | null;
  readonly error: unknown;
}

export function useGuildMeta(): MetaState {
  const [state, setState] = useState<MetaState>({ meta: null, error: null });
  useEffect(() => {
    let live = true;
    loadGuildMeta().then(
      (meta) => {
        if (live) setState({ meta, error: null });
      },
      (error: unknown) => {
        if (live) setState({ meta: null, error });
      },
    );
    return () => {
      live = false;
    };
  }, []);
  return state;
}

export interface GuildBrief {
  readonly id: number;
  readonly name: string;
  readonly logo: number;
  readonly level: number;
}

const TTL_MS = 30_000;
let index: { readonly at: number; readonly rows: Promise<ReadonlyMap<number, GuildBrief>> } | null =
  null;

/** Every guild by id (cached for 30 s; a failed read is forgotten). */
export function loadGuildIndex(now = Date.now()): Promise<ReadonlyMap<number, GuildBrief>> {
  if (index && now - index.at < TTL_MS) return index.rows;
  const rows = fetchPage<GuildRow>('/admin/api/guilds', { limit: 500 }).then(
    (page) =>
      new Map(
        page.items.map((g): [number, GuildBrief] => [
          g.id,
          {
            id: g.id,
            name: g.named ? g.name : `(unnamed #${String(g.id)})`,
            logo: g.logo,
            level: g.level,
          },
        ]),
      ),
  );
  const entry = { at: now, rows };
  index = entry;
  rows.catch(() => {
    if (index === entry) index = null;
  });
  return rows;
}

/** The guild index as React state (empty until it loads, and when the viewer may not list guilds). */
export function useGuildIndex(): ReadonlyMap<number, GuildBrief> {
  const [map, setMap] = useState<ReadonlyMap<number, GuildBrief>>(new Map());
  useEffect(() => {
    let live = true;
    loadGuildIndex().then(
      (m) => {
        if (live) setMap(m);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  return map;
}
