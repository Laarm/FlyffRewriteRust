// The controller of the live map (Plan/25 section 3.2, 3.8, 3.9, 3.14): it loads the worlds and their metadata, owns the subscription lifecycle
// (shown, hidden, idle), turns the user's intent (select, follow, find, jump to a place, toggles) into engine and feed calls, follows a player across
// worlds and layers through the roster, and keeps the last view per browser. The engine draws; the interface (`MapView.tsx`) reads `ui` and calls this.
import type { Subscription } from '../api/ws';
import { Tier } from '../../gen/proto/admin';
import { type Ent } from './entities';
import type { MapEngine } from './engine';
import { MapFeed, IDLE_MS, type FeedSocket, type FeedState } from './feed';
import type { EntityInfo, MapApi, MapContextEvent, PickedPoint } from './api';
import type { FindDoc, FindHit, MetaDoc, WorldsDoc } from './metaTypes';
import {
  DEFAULT_WORLD,
  defaultUpp,
  LAST_VIEW_KEY,
  MAP_PARAM_KEYS,
  defaultCentre,
  fallbackWorld,
  formatSel,
  paramsOfPlace,
  parseSel,
  parseStoredPlace,
  placeFromParams,
  scaleForWorld,
  type Place,
} from '../components/mapPlace';
import type { Highlight } from './overlays';
import { RosterFeed, type RosterSocket } from './roster';
import type {
  FollowTarget,
  KindToggles,
  MapStore,
  MapUiState,
  MonsterFilter,
  SidebarTab,
  ToolId,
} from './state';
import { kindsOf } from './state';
import { Kind, kindsMask } from './types';
import { clampRectToWorld, subscriptionRect, viewRect, type Rect } from './view';
import type { GameAssets } from './gameAssets';
import { loadAtlas } from './atlas';
import type { OverlayToggles } from './overlays';

export interface SessionDeps {
  readonly engine: MapEngine;
  readonly socket: () => FeedSocket &
    RosterSocket & { onPong?: (fn: (rttMs: number) => void) => () => void };
  readonly loadWorlds: (opts?: { fresh?: boolean }) => Promise<WorldsDoc>;
  readonly loadMeta: (id: number) => Promise<MetaDoc>;
  readonly find: (q: string) => Promise<FindDoc>;
  readonly loadAssets: (meta: MetaDoc) => Promise<GameAssets>;
  readonly storage: { get(key: string): string | null; set(key: string, value: string): unknown };
  readonly toast: {
    info(message: string): unknown;
    error(error: unknown, prefix?: string): unknown;
  };
  /** Writes the map's keys into the hash of the console (replace, no history entry). */
  readonly writeRoute: (params: Readonly<Record<string, string | undefined>>) => void;
  /** The character id of the acting GM character, `0` when there is none. */
  readonly actingCharacter: () => number;
  readonly now?: () => number;
  readonly setTimeout?: (fn: () => void, ms: number) => unknown;
  readonly clearTimeout?: (id: unknown) => void;
}

/** The text of a refused subscription. */
export function refusedText(code: number): string {
  switch (code) {
    case 1:
      return 'Your account may not open the live map (it needs view.map).';
    case 2:
      return 'The server has too many open maps right now. Close another map and try again.';
    case 3:
      return 'The server did not accept this view. Zoom or pan and it will ask again.';
    case 4:
      return 'The server does not have this world or layer.';
    default:
      return 'The server refused the live view.';
  }
}

/** The pending "pick a point" request. */
interface PickRequest {
  readonly prompt: string;
  readonly resolve: (p: PickedPoint | null) => void;
}

export class MapSession implements MapApi {
  readonly store: MapStore;
  readonly roster = new RosterFeed();
  readonly #d: SessionDeps;
  readonly #engine: MapEngine;
  readonly #feed: MapFeed;
  #attached = false;
  #visible = true;
  #lastInput = 0;
  #idleTimer: unknown = null;
  #goToken = 0;
  #assetsFor = 0;
  #pick: PickRequest | null = null;
  #contextListeners = new Set<(e: MapContextEvent) => unknown>();
  #favouriteListeners = new Set<(index: number) => void>();
  #highlights = new Map<string, readonly Highlight[]>();
  #rosterOff: (() => void) | null = null;
  #pongOff: (() => void) | null = null;
  #saveTimer: unknown = null;
  #lastWritten = '';
  #missingSince = 0;
  #followCheck: unknown = null;
  #started = false;
  #atlasLoad: Promise<void> | null = null;

  constructor(deps: SessionDeps) {
    this.#d = deps;
    this.#engine = deps.engine;
    this.store = deps.engine.ui;
    this.#feed = new MapFeed({
      socket: deps.socket,
      onFrame: (frame, at) => {
        this.#engine.store.apply(frame, at);
        this.#engine.markDirty(false);
        this.#followWatch();
      },
      onState: (s) => {
        this.#feedState(s);
      },
      onLatency: (ms) => {
        this.#set({ latencyMs: Math.round(ms) });
      },
      onResync: () => {
        // the server restarts the stream from a keyframe: nothing to do here
      },
      now: deps.now ?? (() => Date.now()),
    });
    this.#engine.onView = () => {
      this.syncFeed();
      this.#scheduleSave();
    };
    this.#engine.guildOf = (objId) => this.roster.byObj(objId)?.guildId ?? 0;
  }

  get ui(): MapUiState {
    return this.store.getState();
  }

  /** `start` ran (the first world was asked for). */
  get started(): boolean {
    return this.#started;
  }

  #set(p: Partial<MapUiState>): void {
    this.store.setState(p);
  }

  #now(): number {
    return this.#d.now?.() ?? Date.now();
  }

  #timeout(fn: () => void, ms: number): unknown {
    return (this.#d.setTimeout ?? ((f, t) => setTimeout(f, t)))(fn, ms);
  }

  #clear(id: unknown): void {
    (
      this.#d.clearTimeout ??
      ((i) => {
        clearTimeout(i as ReturnType<typeof setTimeout>);
      })
    )(id);
  }

  // ---- lifecycle: shown, hidden, idle ---------------------------------------------------------------------------------------------------

  /** The map is on screen: the roster stream opens, the subscription follows once a world is loaded. */
  attach(): void {
    if (this.#attached) return;
    this.#attached = true;
    this.#visible = typeof document === 'undefined' || document.visibilityState !== 'hidden';
    this.#lastInput = this.#now();
    this.#set({ paused: false });
    this.#engine.start();
    if (typeof document !== 'undefined')
      document.addEventListener('visibilitychange', this.#onVisibility);
    this.#idleTimer = this.#interval();
    this.#openRoster();
    this.syncFeed();
  }

  /** The map left the screen: every stream closes at once. */
  detach(): void {
    if (!this.#attached) return;
    this.#attached = false;
    if (typeof document !== 'undefined')
      document.removeEventListener('visibilitychange', this.#onVisibility);
    if (this.#idleTimer !== null) clearInterval(this.#idleTimer as ReturnType<typeof setInterval>);
    this.#idleTimer = null;
    if (this.#saveTimer !== null) this.#clear(this.#saveTimer);
    this.#saveTimer = null;
    this.#save();
    this.#feed.close();
    this.#closeRoster();
    this.#engine.stop();
    this.cancelPick();
    this.#set({ hover: null });
  }

  /** Every 15 s: the idle check, and the online counts of the world selector every 30 s while the map is live. */
  #interval(): unknown {
    let ticks = 0;
    return setInterval(() => {
      if (this.#now() - this.#lastInput >= IDLE_MS && !this.ui.paused) this.pause();
      ticks += 1;
      if (ticks % 2 === 0 && this.#live) this.#reloadWorlds();
    }, 15_000);
  }

  #reloadWorlds(): void {
    void this.#d
      .loadWorlds({ fresh: true })
      .then((worlds) => {
        this.#set({ worlds });
      })
      .catch(() => undefined);
  }

  readonly #onVisibility = (): void => {
    this.#visible = document.visibilityState !== 'hidden';
    if (this.#visible) {
      this.#lastInput = this.#now();
      this.#openRoster();
      this.syncFeed();
    } else {
      this.#feed.close();
      this.#closeRoster();
    }
  };

  /** Any input of the user in the window: the idle clock restarts. */
  touch(): void {
    this.#lastInput = this.#now();
  }

  pause(): void {
    if (this.ui.paused) return;
    this.#set({ paused: true });
    this.#feed.close();
    this.#closeRoster();
  }

  resume(): void {
    if (!this.ui.paused) return;
    this.#lastInput = this.#now();
    this.#set({ paused: false });
    this.#openRoster();
    this.syncFeed();
  }

  get #live(): boolean {
    const ui = this.ui;
    const v = this.#engine.view;
    return (
      this.#attached &&
      this.#visible &&
      !ui.paused &&
      ui.meta !== null &&
      ui.worldId !== 0 &&
      v.w > 0 &&
      v.h > 0
    );
  }

  #openRoster(): void {
    if (this.roster.open || !this.#attached || !this.#visible || this.ui.paused) return;
    this.roster.start(this.#d.socket());
    this.#rosterOff?.();
    this.#rosterOff = this.roster.onChange(() => {
      this.#set({ rosterLive: this.roster.allowed });
      this.#rosterChanged();
    });
    const sock = this.#d.socket();
    this.#pongOff?.();
    this.#pongOff = sock.onPong
      ? sock.onPong((ms) => {
          this.#set({ latencyMs: Math.round(ms) });
        })
      : null;
  }

  #closeRoster(): void {
    this.roster.stop();
    this.#rosterOff?.();
    this.#rosterOff = null;
    this.#pongOff?.();
    this.#pongOff = null;
  }

  /** The socket was replaced (a sign-in as staff): subscribe again on the new one. */
  reopen(): void {
    this.#feed.close();
    this.#closeRoster();
    this.#openRoster();
    this.syncFeed();
  }

  // ---- the subscription -----------------------------------------------------------------------------------------------------------------

  /** Sends what the view wants to the server (opens the subscription, or updates it with the hysteresis of `feed.ts`). */
  syncFeed(): void {
    if (!this.#live) {
      if (this.#feed.open && (!this.#attached || !this.#visible || this.ui.paused))
        this.#feed.close();
      return;
    }
    const ui = this.ui;
    const v = this.#engine.view;
    // nothing exists outside the world: the rectangle is cut to it (a small dungeon on a wide canvas then still gets tier A)
    const extent = ui.meta?.geometry.extent_m;
    const cut = (r: Rect): Rect => (extent ? clampRectToWorld(r, extent) : r);
    this.#feed.want(
      {
        world: ui.worldId,
        layer: ui.layer,
        rect: cut(subscriptionRect(v)),
        kinds: kindsMask(kindsOf(ui.kinds)),
        maxTier: Tier.A,
      },
      cut(viewRect(v, 0)),
    );
  }

  #feedState(s: FeedState): void {
    this.#engine.stats.subscribed = this.#feed.open;
    if (s.kind === 'refused') this.#set({ feed: 'refused', feedMessage: refusedText(s.code) });
    else this.#set({ feed: s.kind, feedMessage: '' });
  }

  // ---- worlds and places ---------------------------------------------------------------------------------------------------------------

  /** Opens the map: the world list, then the place of the route, the last view of this browser, or Flaris. */
  async start(route: Readonly<Record<string, string>>): Promise<void> {
    this.#started = true;
    let worlds: WorldsDoc;
    try {
      worlds = await this.#d.loadWorlds();
    } catch (e) {
      this.#set({ phase: 'error', error: e instanceof Error ? e.message : String(e) });
      return;
    }
    this.#set({ worlds });
    const fromRoute = placeFromParams(route);
    const stored = parseStoredPlace(this.#d.storage.get(LAST_VIEW_KEY));
    const place = fromRoute ?? stored ?? { world: DEFAULT_WORLD, layer: 0 };
    const sel = parseSel(route['sel']);
    await this.goTo({ ...place, world: fallbackWorld(worlds, place.world) }, { selection: sel });
  }

  /** The route changed under the map (a palette hit, a pasted link): follows it unless it is the map's own echo. */
  async applyRoute(route: Readonly<Record<string, string>>): Promise<void> {
    if (!this.#started) return;
    const written = this.#routeKey(route);
    if (written === this.#lastWritten) return;
    const place = placeFromParams(route);
    if (!place) return;
    this.#lastWritten = written;
    await this.goTo(place, { selection: parseSel(route['sel']), fly: true });
  }

  #routeKey(params: Readonly<Record<string, string>>): string {
    return MAP_PARAM_KEYS.map((k) => `${k}=${params[k] ?? ''}`).join('&');
  }

  /**
   * Opens a world and layer, with the camera at (x, z) and the scale `upp` when the place has them. A place without a centre opens the default
   * view of the world (Flaris in Madrigal, the whole world elsewhere). Resolves when the world is on screen.
   */
  async goTo(
    place: Place,
    opts: {
      readonly keepFollow?: boolean;
      readonly selection?: readonly number[];
      readonly fly?: boolean;
    } = {},
  ): Promise<void> {
    const token = ++this.#goToken;
    const before = this.ui;
    let meta: MetaDoc;
    try {
      meta = await this.#d.loadMeta(place.world);
    } catch (e) {
      if (token !== this.#goToken) return;
      if (before.meta === null)
        this.#set({ phase: 'error', error: e instanceof Error ? e.message : String(e) });
      else this.#d.toast.error(e, 'The map could not open this world');
      return;
    }
    if (token !== this.#goToken) return;
    const sameWorld = before.meta !== null && before.meta.id === meta.id;
    const engine = this.#engine;
    if (!sameWorld) {
      engine.setWorld(meta);
      this.#ensureAssets(meta);
    } else if (before.layer !== place.layer) {
      engine.store.clear();
      engine.trails.clear();
      engine.heat.clear(Date.now());
    }
    const size = engine.view;
    const centre =
      place.x !== undefined && place.z !== undefined ? { x: place.x, z: place.z } : null;
    let upp = place.upp ?? (sameWorld ? size.upp : undefined);
    const dungeon = meta.tiles === null || meta.geometry.extent_m[0] <= 2048;
    upp ??= dungeon ? scaleForWorld(meta, size.w, size.h) : defaultUpp(size.w);
    const wide = upp === scaleForWorld(meta, size.w, size.h);
    const target =
      centre ??
      (sameWorld
        ? { x: size.cx, z: size.cz }
        : wide
          ? { x: meta.geometry.extent_m[0] / 2, z: meta.geometry.extent_m[1] / 2 }
          : worldCentre(meta));
    // a worldwide jump with a flight would sweep across the map: the world is already shown, so fly only inside it
    if (opts.fly === true && sameWorld) engine.flyTo(target.x, target.z, upp);
    else engine.setView({ cx: target.x, cz: target.z, upp });
    const keep = opts.keepFollow === true;
    this.#set({
      phase: 'ready',
      error: null,
      meta,
      worldId: meta.id,
      layer: place.layer,
      ...(keep ? {} : { follow: null }),
      layerClosed: false,
    });
    if (!keep) engine.setFollow(null);
    if (opts.selection) this.#setSelection(opts.selection);
    else if (!keep) this.#setSelection([]);
    engine.markDirty();
    this.#ensureAtlas();
    this.syncFeed();
    this.#save();
  }

  /** Madrigal's artworks load the first time the layer is on in Madrigal, never before. */
  #ensureAtlas(): void {
    if (this.#atlasLoad !== null || !this.ui.toggles.atlas || this.ui.worldId !== DEFAULT_WORLD)
      return;
    this.#atlasLoad = loadAtlas().then(
      (images) => {
        this.#engine.setAtlasImages(images);
      },
      () => undefined,
    );
  }

  /** Loads the names and pictures once per world (the monsters' names come from the bundle, the hints of the meta stand in meanwhile). */
  #ensureAssets(meta: MetaDoc): void {
    if (this.#engine.assets !== null && this.#assetsFor !== 0) return;
    this.#assetsFor = meta.id;
    void this.#d
      .loadAssets(meta)
      .then((a) => {
        this.#engine.setAssets(a);
      })
      .catch(() => {
        this.#assetsFor = 0;
      });
  }

  /** Opens a place (the locations panel, the palette): the layer defaults to 0. */
  flyTo(place: Place): Promise<void> {
    return this.goTo(place, { fly: true });
  }

  /** Moves the camera to (x, z) without a flight (the minimap drag). */
  centreOn(x: number, z: number): void {
    this.#engine.setView({ cx: x, cz: z });
  }

  /** Switches the layer of the current world. */
  setLayer(layer: number): Promise<void> {
    return this.goTo({ world: this.ui.worldId, layer });
  }

  // ---- last view and the route ---------------------------------------------------------------------------------------------------------

  #scheduleSave(): void {
    if (this.#saveTimer !== null) return;
    this.#saveTimer = this.#timeout(() => {
      this.#saveTimer = null;
      this.#save();
    }, 600);
  }

  #save(): void {
    const ui = this.ui;
    const v = this.#engine.view;
    if (ui.worldId === 0 || v.w <= 0) return;
    const place = { world: ui.worldId, layer: ui.layer, x: v.cx, z: v.cz, upp: v.upp };
    this.#d.storage.set(LAST_VIEW_KEY, JSON.stringify(place));
    const params: Record<string, string | undefined> = {
      ...paramsOfPlace(place),
      sel: this.#selectionParam(),
    };
    const key = this.#routeKey(
      Object.fromEntries(Object.entries(params).filter(([, x]) => x !== undefined)) as Record<
        string,
        string
      >,
    );
    if (key === this.#lastWritten) return;
    this.#lastWritten = key;
    this.#d.writeRoute(params);
  }

  #selectionParam(): string | undefined {
    const items: { id: number; kind: number }[] = [];
    for (const id of this.#engine.selected) {
      const e = this.#engine.store.byId.get(id);
      if (e) items.push({ id, kind: e.kind });
    }
    return items.length === 0 ? undefined : formatSel(items);
  }

  // ---- selection and follow ------------------------------------------------------------------------------------------------------------

  #setSelection(ids: readonly number[]): void {
    this.#engine.setSelection(ids);
    this.#set({ selection: [...this.#engine.selected] });
    this.#scheduleSave();
  }

  select(ids: readonly number[], mode: 'set' | 'add' | 'toggle' = 'set'): void {
    if (mode === 'set') {
      this.#setSelection(ids);
      return;
    }
    const next = new Set(this.#engine.selected);
    for (const id of ids) {
      if (mode === 'toggle' && next.has(id)) next.delete(id);
      else next.add(id);
    }
    this.#setSelection([...next]);
  }

  clearSelection(): void {
    this.#setSelection([]);
  }

  /** Selects the next (`1`) or previous (`-1`) entity in view, players first (keys N and Shift+N). */
  selectNext(dir: 1 | -1): void {
    const list = this.#engine.lastResult?.inCanvas ?? [];
    if (list.length === 0) return;
    const sorted = [...list].sort((a, b) => rank(a) - rank(b) || a.id - b.id);
    const cur = this.ui.selection[0];
    const at = cur === undefined ? -1 : sorted.findIndex((e) => e.id === cur);
    const next = sorted[(at + dir + sorted.length) % sorted.length];
    if (next) this.#setSelection([next.id]);
  }

  /** Follow: `null` stops; a player is followed across worlds and layers. */
  follow(id: number | null): void {
    if (id === null) {
      this.#engine.setFollow(null);
      this.#set({ follow: null });
      return;
    }
    const e = this.#engine.store.byId.get(id);
    const row = this.roster.byObj(id);
    const target: FollowTarget = {
      objId: id,
      characterId: row?.characterId ?? 0,
      name: e ? this.#engine.nameOf(e) : (row?.name ?? `#${String(id)}`),
    };
    this.#missingSince = 0;
    this.#engine.setFollow(id);
    this.#set({ follow: target });
  }

  /** The follow key: stops following, or follows the first selected entity. */
  toggleFollow(): void {
    if (this.ui.follow) {
      this.follow(null);
      return;
    }
    const first = this.ui.selection[0];
    if (first !== undefined) this.follow(first);
  }

  /** The user dragged the map: follow ends (Plan/25 section 3.9). */
  dragged(): void {
    if (this.ui.follow) this.follow(null);
  }

  #followWatch(): void {
    const f = this.ui.follow;
    if (!f) return;
    if (this.#engine.store.byId.has(f.objId)) {
      this.#missingSince = 0;
      return;
    }
    if (this.#missingSince === 0) this.#missingSince = this.#now();
    if (this.#followCheck === null) {
      this.#followCheck = this.#timeout(() => {
        this.#followCheck = null;
        this.#followLost();
      }, 3000);
    }
  }

  /** The followed entity is not in the frames: a player that went offline or a monster that died ends the follow with a toast. */
  #followLost(): void {
    const f = this.ui.follow;
    if (!f || this.#engine.store.byId.has(f.objId)) return;
    const row = f.characterId !== 0 ? this.roster.rows.get(f.characterId) : undefined;
    if (row?.world === this.ui.worldId && row.layer === this.ui.layer) {
      // still here, outside the subscription for the moment: the camera goes to the roster position
      this.#engine.setView({ cx: row.x, cz: row.z });
      return;
    }
    if (row) return; // in another world: the roster handler follows it
    this.#d.toast.info(f.characterId !== 0 ? `${f.name} went offline.` : `${f.name} is gone.`);
    this.follow(null);
  }

  /** A roster delta: a followed player that changed world or layer takes the map with it (a toast says where). */
  #rosterChanged(): void {
    // the acting character gets its "You" ring and its label priority
    const acting = this.#d.actingCharacter();
    const objId = acting === 0 ? 0 : (this.roster.rows.get(acting)?.objId ?? 0);
    if (objId !== this.#engine.actingObjId) {
      this.#engine.actingObjId = objId;
      this.#engine.markDirty();
    }
    const f = this.ui.follow;
    if (!f || f.characterId === 0) return;
    const row = this.roster.rows.get(f.characterId);
    if (!row) return;
    if (row.objId !== f.objId) {
      this.#set({ follow: { ...f, objId: row.objId } });
      this.#engine.setFollow(row.objId);
    }
    if (row.world !== this.ui.worldId || row.layer !== this.ui.layer) {
      const world = this.ui.worlds?.worlds.find((w) => w.id === row.world);
      const where = world ? world.name : `world ${String(row.world)}`;
      this.#d.toast.info(
        `${f.name} entered ${where}${row.layer === 0 ? '' : ` (layer ${String(row.layer)})`}.`,
      );
      void this.goTo(
        { world: row.world, layer: row.layer, x: row.x, z: row.z },
        { keepFollow: true, selection: [...this.#engine.selected] },
      );
    }
  }

  // ---- find ---------------------------------------------------------------------------------------------------------------------------

  async find(q: string): Promise<readonly FindHit[]> {
    const doc = await this.#d.find(q);
    return doc.hits;
  }

  /** Switches the world and layer when the hit is elsewhere, flies there and selects a live hit. */
  async reveal(hit: FindHit): Promise<void> {
    const live = hit.obj_id !== 0;
    const upp = live ? (hit.kind === 'player' || hit.kind === 'monster' ? 1 : 2) : undefined;
    await this.goTo(
      {
        world: hit.world,
        layer: hit.layer,
        x: hit.x,
        z: hit.z,
        ...(upp === undefined ? {} : { upp }),
      },
      { fly: true, ...(live ? { selection: [hit.obj_id] } : {}) },
    );
  }

  // ---- toggles and tools -------------------------------------------------------------------------------------------------------------

  setToggle(key: keyof OverlayToggles, value: boolean): void {
    this.#set({ toggles: { ...this.ui.toggles, [key]: value } });
    if (key === 'atlas') this.#ensureAtlas();
    if (key === 'heatmap') {
      if (value) this.#engine.heat.clear(Date.now());
      this.#set({ heatSince: value ? this.#engine.heat.since : null });
    }
    this.#engine.markDirty();
  }

  setKind(key: keyof KindToggles, value: boolean): void {
    this.#set({ kinds: { ...this.ui.kinds, [key]: value } });
    this.#engine.markDirty();
    this.syncFeed();
  }

  setMonsterFilter(key: keyof MonsterFilter, value: boolean): void {
    this.#set({ monsterFilter: { ...this.ui.monsterFilter, [key]: value } });
    this.#engine.markDirty();
  }

  setFilterText(text: string): void {
    const parsed = this.#engine.setFilterText(text);
    this.#set({ filterText: text, filterErrors: parsed.errors });
  }

  setTool(tool: ToolId): void {
    if (tool !== 'ruler') {
      this.#set({ ruler: null });
      this.#engine.setRuler(null);
    }
    this.#set({ tool });
  }

  setTab(tab: SidebarTab): void {
    this.#set({ tab, sidebarOpen: true });
  }

  setSidebarOpen(open: boolean): void {
    this.#set({ sidebarOpen: open });
  }

  /** Asks the world list again (the online counts) and the server for a fresh keyframe. */
  refresh(): void {
    this.#reloadWorlds();
    this.#feed.close();
    this.syncFeed();
  }

  /** The ruler: the first click sets a, the second b, the third starts over. */
  rulerPoint(x: number, z: number): void {
    const r = this.ui.ruler;
    const next: MapUiState['ruler'] =
      r?.b === null ? { a: r.a, b: [x, z] } : { a: [x, z], b: null };
    this.#set({ ruler: next });
    this.#engine.setRuler(next);
  }

  /** The ruler's second point follows the cursor until it is set. */
  rulerPreview(x: number, z: number): void {
    const r = this.ui.ruler;
    if (r?.b === null) this.#engine.setRuler({ a: r.a, b: [x, z] });
  }

  // ---- point picking, context menu, highlights -----------------------------------------------------------------------------------------

  pickPoint(prompt: string): Promise<PickedPoint | null> {
    this.cancelPick();
    return new Promise((resolve) => {
      this.#pick = { prompt, resolve };
      this.#set({ picking: prompt, tool: 'pick' });
    });
  }

  /** `true` when the click was taken by a pending pick request. */
  pickAt(x: number, z: number): boolean {
    const p = this.#pick;
    if (!p) return false;
    this.#pick = null;
    this.#set({ picking: null, tool: 'pan' });
    p.resolve({ world: this.ui.worldId, layer: this.ui.layer, x, z });
    return true;
  }

  cancelPick(): void {
    const p = this.#pick;
    if (!p) return;
    this.#pick = null;
    this.#set({ picking: null, tool: 'pan' });
    p.resolve(null);
  }

  onContextMenu(fn: (e: MapContextEvent) => unknown): () => void {
    this.#contextListeners.add(fn);
    return () => this.#contextListeners.delete(fn);
  }

  /** A right click: `true` when a listener took it. */
  contextMenu(e: MapContextEvent): boolean {
    let handled = false;
    for (const fn of [...this.#contextListeners]) if (fn(e) === true) handled = true;
    return handled;
  }

  onFavourite(fn: (index: number) => void): () => void {
    this.#favouriteListeners.add(fn);
    return () => this.#favouriteListeners.delete(fn);
  }

  favouriteKey(index: number): void {
    for (const fn of [...this.#favouriteListeners]) fn(index);
  }

  setHighlights(key: string, items: readonly Highlight[]): void {
    if (items.length === 0) this.#highlights.delete(key);
    else this.#highlights.set(key, items);
    this.#engine.setHighlights([...this.#highlights.values()].flat());
  }

  jumpToMe(): boolean {
    const id = this.#d.actingCharacter();
    if (id === 0) return false;
    const row = this.roster.rows.get(id);
    if (!row) return false;
    void this.goTo({ world: row.world, layer: row.layer, x: row.x, z: row.z }, { fly: true });
    return true;
  }

  // ---- snapshots for other views --------------------------------------------------------------------------------------------------------

  meta(): MetaDoc | null {
    return this.ui.meta;
  }

  entity(id: number): EntityInfo | null {
    const e = this.#engine.store.byId.get(id);
    return e ? this.#info(e) : null;
  }

  inView(kind?: number): EntityInfo[] {
    const list = this.#engine.lastResult?.inCanvas ?? [];
    return list.filter((e) => kind === undefined || e.kind === kind).map((e) => this.#info(e));
  }

  #info(e: Ent): EntityInfo {
    const row = e.kind === Kind.PLAYER ? this.roster.byObj(e.id) : undefined;
    return {
      id: e.id,
      kind: e.kind,
      sub: e.sub,
      prop: e.prop,
      level: e.level,
      name: this.#engine.nameOf(e),
      x: e.x1,
      z: e.z1,
      hpPct: e.hp,
      flags: e.flags,
      target: e.target,
      characterId: row?.characterId ?? 0,
      guildId: row?.guildId ?? 0,
    };
  }

  /** `layerClosed`: the layer the map shows is not in the live list any more (the layers stream calls this). */
  layersChanged(live: readonly { world: number; layer: number }[]): void {
    const ui = this.ui;
    const closed =
      ui.layer !== 0 &&
      ui.worldId !== 0 &&
      !live.some((l) => l.world === ui.worldId && l.layer === ui.layer);
    if (ui.layerClosed !== closed) this.#set({ layerClosed: closed });
  }

  dispose(): void {
    this.detach();
    this.#goToken += 1;
    if (this.#followCheck !== null) this.#clear(this.#followCheck);
    this.#followCheck = null;
  }
}

function rank(e: Ent): number {
  switch (e.kind) {
    case Kind.PLAYER:
      return 0;
    case Kind.MONSTER:
      return e.sub >= 2 ? 1 : 2;
    case Kind.NPC:
      return 3;
    default:
      return 4;
  }
}

/** The middle of a world; the default view of Madrigal is the Flaris point of its data. */
function worldCentre(meta: MetaDoc): { x: number; z: number } {
  return defaultCentre(meta);
}

/** Tests and the viewport rectangle of a view (re-exported so that callers need no second import). */
export type { Rect, Subscription };
