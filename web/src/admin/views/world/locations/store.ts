// The state of the quick locations (Plan/25 section 3.13): the merged list of the server (`GET /admin/api/locations`), and the two things that stay in
// the browser: the favourites (up to nine, keys 1 to 9) and the ten most recent jumps, both kept as ids in `localStorage`. The list is loaded once per
// page and again after a change or every 30 s while a view that shows it is mounted (a bookmark of another staff member appears by itself).
import { useEffect } from 'react';
import { create } from 'zustand';
import { safeStorage } from '../../../../platform/storage';
import { fetchLocations, type LocationRow, type LocationsDoc } from './api';

export const FAVOURITES_KEY = 'flyff.admin.locations.favourites.v1';
export const RECENT_KEY = 'flyff.admin.locations.recent.v1';
/** The group a bookmark without a group lands in (the server uses the same). */
export const BOOKMARK_GROUP = 'Bookmarks';
/** The groups of the panel that are open (ids), kept per browser. */
export const OPEN_KEY = 'flyff.admin.locations.open.v1';
export const MAX_FAVOURITES = 9;
export const MAX_RECENT = 10;
const POLL_MS = 30_000;

function readIds(key: string, limit: number): string[] {
  try {
    const raw = safeStorage.local.get(key);
    const v: unknown = raw === null ? [] : JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    const out: string[] = [];
    for (const e of v) if (typeof e === 'string' && !out.includes(e)) out.push(e);
    return out.slice(0, limit);
  } catch {
    return [];
  }
}

function writeIds(key: string, ids: readonly string[]): void {
  safeStorage.local.set(key, JSON.stringify(ids));
}

export interface LocationsState {
  readonly doc: LocationsDoc | null;
  readonly status: 'idle' | 'loading' | 'ready' | 'error';
  readonly error: string | null;
  readonly favourites: readonly string[];
  readonly recent: readonly string[];
  /** Loads the list (again when `force`); concurrent calls share one request. */
  load: (force?: boolean) => Promise<void>;
  /** Adds or removes a favourite; `false` when the nine places are taken. */
  toggleFavourite: (id: string) => boolean;
  /** Moves a favourite to another place (drag and drop). */
  moveFavourite: (id: string, toIndex: number) => void;
  recordJump: (id: string) => void;
  forgetRecent: (id: string) => void;
}

let inflight: Promise<void> | null = null;

export const useLocations = create<LocationsState>()((set, get) => ({
  doc: null,
  status: 'idle',
  error: null,
  favourites: readIds(FAVOURITES_KEY, MAX_FAVOURITES),
  recent: readIds(RECENT_KEY, MAX_RECENT),
  load(force = false) {
    if (inflight) return inflight;
    if (!force && get().status === 'ready') return Promise.resolve();
    if (get().doc === null) set({ status: 'loading', error: null });
    const run = fetchLocations().then(
      (doc) => {
        set({ doc, status: 'ready', error: null });
      },
      (error: unknown) => {
        set({
          status: get().doc === null ? 'error' : 'ready',
          error: error instanceof Error ? error.message : String(error),
        });
      },
    );
    inflight = run.finally(() => {
      inflight = null;
    });
    return inflight;
  },
  toggleFavourite(id) {
    const now = get().favourites;
    if (now.includes(id)) {
      const next = now.filter((f) => f !== id);
      writeIds(FAVOURITES_KEY, next);
      set({ favourites: next });
      return true;
    }
    if (now.length >= MAX_FAVOURITES) return false;
    const next = [...now, id];
    writeIds(FAVOURITES_KEY, next);
    set({ favourites: next });
    return true;
  },
  moveFavourite(id, toIndex) {
    const now = get().favourites;
    if (!now.includes(id)) return;
    const rest = now.filter((f) => f !== id);
    const at = Math.max(0, Math.min(rest.length, toIndex));
    const next = [...rest.slice(0, at), id, ...rest.slice(at)];
    writeIds(FAVOURITES_KEY, next);
    set({ favourites: next });
  },
  recordJump(id) {
    const next = [id, ...get().recent.filter((r) => r !== id)].slice(0, MAX_RECENT);
    writeIds(RECENT_KEY, next);
    set({ recent: next });
  },
  forgetRecent(id) {
    const next = get().recent.filter((r) => r !== id);
    writeIds(RECENT_KEY, next);
    set({ recent: next });
  },
}));

/** The row with this id, if the list has it. */
export function rowById(doc: LocationsDoc | null, id: string): LocationRow | undefined {
  return doc?.locations.find((l) => l.id === id);
}

/** Loads the list when a view mounts and keeps it fresh while the view is there. */
export function useLocationsFeed(): void {
  const load = useLocations((s) => s.load);
  useEffect(() => {
    void load();
    const t = setInterval(() => {
      if (document.visibilityState !== 'hidden') void load(true);
    }, POLL_MS);
    return () => {
      clearInterval(t);
    };
  }, [load]);
}

/** Tests: forgets the list and the browser's choices. */
export function resetLocationsForTests(): void {
  inflight = null;
  safeStorage.local.remove(FAVOURITES_KEY);
  safeStorage.local.remove(RECENT_KEY);
  safeStorage.local.remove(OPEN_KEY);
  useLocations.setState({ doc: null, status: 'idle', error: null, favourites: [], recent: [] });
}
