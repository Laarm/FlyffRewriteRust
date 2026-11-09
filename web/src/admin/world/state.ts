// The state of the map that the interface shows and that other views (the inspector, the tools, the locations panel) read: one vanilla zustand
// store per map engine (`useStore(api.store, selector)` in a component). The engine writes it at a few hertz at most (never per frame); the
// pure data of the entities stays in the engine's `EntityStore`.
import { createStore, type StoreApi } from 'zustand/vanilla';
import { DEFAULT_TOGGLES, type OverlayToggles } from './overlays';
import type { MetaDoc, WorldsDoc } from './metaTypes';

export type ToolId = 'pan' | 'select' | 'ruler' | 'pick';
export type SidebarTab = 'inspector' | 'layers' | 'lists' | 'tools';

/** Which entity kinds the map asks the server for (and draws). */
export interface KindToggles {
  readonly players: boolean;
  readonly monsters: boolean;
  readonly npcs: boolean;
  readonly items: boolean;
  readonly pets: boolean;
  readonly ctrls: boolean;
}

export const DEFAULT_KINDS: KindToggles = {
  players: true,
  monsters: true,
  npcs: true,
  items: true,
  pets: false,
  ctrls: false,
};

/** The monster rank and state filters of the Layers tab (client side only). */
export interface MonsterFilter {
  readonly normal: boolean;
  readonly captain: boolean;
  readonly boss: boolean;
  readonly awake: boolean;
  readonly asleep: boolean;
}

export const DEFAULT_MONSTER_FILTER: MonsterFilter = {
  normal: true,
  captain: true,
  boss: true,
  awake: true,
  asleep: true,
};

export interface ViewSummary {
  readonly cx: number;
  readonly cz: number;
  readonly upp: number;
  readonly w: number;
  readonly h: number;
}

export interface Counts {
  readonly players: number;
  readonly monsters: number;
  readonly npcs: number;
  readonly pets: number;
  readonly items: number;
  readonly ctrls: number;
}

export const NO_COUNTS: Counts = { players: 0, monsters: 0, npcs: 0, pets: 0, items: 0, ctrls: 0 };

/** The follow target: the object, and for a player its character id (the roster follows it across worlds and layers). */
export interface FollowTarget {
  readonly objId: number;
  readonly characterId: number;
  readonly name: string;
}

export interface MapUiState {
  readonly phase: 'loading' | 'ready' | 'error';
  readonly error: string | null;
  readonly worlds: WorldsDoc | null;
  readonly worldId: number;
  readonly layer: number;
  readonly meta: MetaDoc | null;
  readonly view: ViewSummary;
  readonly cursor: { readonly x: number; readonly z: number } | null;
  readonly counts: Counts;
  /** Entities the server last sent (in the subscription rectangle). */
  readonly held: number;
  readonly tier: number;
  readonly degraded: boolean;
  readonly feed: 'closed' | 'live' | 'refused';
  readonly feedMessage: string;
  /** Idle for ten minutes: the subscription is closed and "Resume live view" shows. */
  readonly paused: boolean;
  readonly latencyMs: number | null;
  readonly selection: readonly number[];
  readonly follow: FollowTarget | null;
  readonly hover: { readonly id: number; readonly sx: number; readonly sy: number } | null;
  readonly toggles: OverlayToggles;
  readonly kinds: KindToggles;
  readonly monsterFilter: MonsterFilter;
  readonly filterText: string;
  readonly filterErrors: readonly string[];
  readonly tool: ToolId;
  readonly tab: SidebarTab;
  readonly sidebarOpen: boolean;
  /** The ruler's two points. */
  readonly ruler: {
    readonly a: readonly [number, number];
    readonly b: readonly [number, number] | null;
  } | null;
  /** "since HH:MM" of the heatmap, `null` while it is off. */
  readonly heatSince: number | null;
  /** The map is waiting for a point (teleport target): the text of the prompt. */
  readonly picking: string | null;
  /** Roster dots exist (the session may read the roster). */
  readonly rosterLive: boolean;
  /** The layer on screen closed (it is not in the live list any more): "layer closed", offer layer 0. */
  readonly layerClosed: boolean;
}

export type MapStore = StoreApi<MapUiState>;

export function initialUi(): MapUiState {
  return {
    phase: 'loading',
    error: null,
    worlds: null,
    worldId: 0,
    layer: 0,
    meta: null,
    view: { cx: 0, cz: 0, upp: 2, w: 0, h: 0 },
    cursor: null,
    counts: NO_COUNTS,
    held: 0,
    tier: 0,
    degraded: false,
    feed: 'closed',
    feedMessage: '',
    paused: false,
    latencyMs: null,
    selection: [],
    follow: null,
    hover: null,
    toggles: DEFAULT_TOGGLES,
    kinds: DEFAULT_KINDS,
    monsterFilter: DEFAULT_MONSTER_FILTER,
    filterText: '',
    filterErrors: [],
    tool: 'pan',
    tab: 'inspector',
    sidebarOpen: true,
    ruler: null,
    heatSince: null,
    picking: null,
    rosterLive: false,
    layerClosed: false,
  };
}

export function createMapStore(): MapStore {
  return createStore<MapUiState>(() => initialUi());
}

/** The kinds bitset of a subscription for the enabled kinds. */
export function kindsOf(k: KindToggles): readonly number[] {
  const out: number[] = [];
  if (k.players) out.push(1);
  if (k.monsters) out.push(2);
  if (k.npcs) out.push(3);
  if (k.pets) out.push(4);
  if (k.items) out.push(5);
  if (k.ctrls) out.push(6);
  return out;
}
