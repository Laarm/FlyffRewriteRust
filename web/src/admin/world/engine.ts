// The map engine (Plan/25 section 3.2 to 3.9, WP-ADMIN-09): the camera, the base layer (tile plan, cache, GPU), the entity layer (store, culling,
// interpolation, instances), the labels, the overlays, picking and the render loop. It knows nothing of the network: the controller (`session.ts`)
// feeds it frames and a world, and the interface reads `ui`. A frame is drawn only when something changed (a new view, a tile, a slide in progress,
// a pulse), never while the map is idle, and only what lies in the view is touched.
import type { StoreApi } from 'zustand/vanilla';
import { landMeters } from '../components/mapGeo';
import { DEFAULT_WORLD } from '../components/mapPlace';
import {
  compileFilter,
  isActive,
  parseFilter,
  type FilterSubject,
  type ParsedFilter,
} from './filter';
import { type Ent, EntityStore } from './entities';
import { HeatGrid } from './heatmap';
import { buildInstances, type InstanceResult } from './instances';
import { LabelLayer, labelRole, selectLabels, type LabelCandidate } from './labels';
import type { MetaDoc } from './metaTypes';
import {
  drawOverlays,
  readColors,
  FALLBACK_COLORS,
  type AtlasImage,
  type Highlight,
  type OverlayColors,
} from './overlays';
import { pickAt, pickBox } from './picking';
import { type GlLayer, type TileDraw } from './gl';
import type { GameAssets } from './gameAssets';
import type { MapUiState } from './state';
import { Flag, Kind, Rank, isBossRank } from './types';
import { Coverage, TileCache, chooseLevel, planTiles, tileRect, type TileId } from './tiles';
import { createTileLoader, disposeTile, type GpuTile, type TileIo } from './tileTextures';
import { TrailSet } from './trails';
import {
  DRAW_MARGIN_PX,
  clampCentre,
  clampUpp,
  panBy,
  sampleFly,
  screenToWorld,
  startFly,
  upLimits,
  viewRect,
  zoomAt,
  type Fly,
  type Rect,
  type View,
} from './view';
import { buildAtlas } from './spriteAtlas';
import type { StyleContext } from './style';
import { setS3tc } from './decodePool';

/** At most this many GPU textures are held (about 16 MiB as BC1, 128 MiB as RGBA8: Plan/25 section 3.4). */
export const TILE_CAPACITY = 512;
/** Tile loads in flight. */
export const MAX_TILE_LOADS = 6;

/** The counters `window.__adminMapStats` exposes for the tests (Plan/25 section 3.7). */
export interface MapStats {
  /** Sprite instances written for the last frame. */
  instances: number;
  /** Entities placed inside the canvas plus the margin (the instances of the entity layer, without rings, bins and marks). */
  drawn: number;
  labels: number;
  /** Tiles on the GPU. */
  tiles: number;
  tilesRequested: number;
  tilesLoading: number;
  /** Entities the server last sent. */
  entities: number;
  bins: number;
  tier: number;
  frames: number;
  /** The mean cost of the last 60 frames in ms (CPU, without the GPU). */
  frameMs: number;
  world: number;
  layer: number;
  upp: number;
  cx: number;
  cz: number;
  width: number;
  height: number;
  subscribed: boolean;
}

export const EMPTY_STATS: MapStats = {
  instances: 0,
  drawn: 0,
  labels: 0,
  tiles: 0,
  tilesRequested: 0,
  tilesLoading: 0,
  entities: 0,
  bins: 0,
  tier: 0,
  frames: 0,
  frameMs: 0,
  world: 0,
  layer: 0,
  upp: 0,
  cx: 0,
  cz: 0,
  width: 0,
  height: 0,
  subscribed: false,
};

export interface EngineDeps {
  readonly gl: GlLayer;
  readonly overlay: HTMLCanvasElement;
  readonly labelHost: HTMLElement;
  readonly io: TileIo;
  readonly ui: StoreApi<MapUiState>;
  readonly now?: () => number;
  readonly raf?: (cb: FrameRequestCallback) => number;
  readonly caf?: (id: number) => void;
}

/** What the controller wants drawn on top (the "You" marker, a spawn region highlighted by the inspector). */
export interface EngineExtras {
  highlights: readonly Highlight[];
  atlas: readonly AtlasImage[];
}

function subjectOf(
  e: Ent,
  store: EntityStore,
  assets: GameAssets | null,
  guild: (id: number) => number,
): FilterSubject {
  const name =
    e.kind === Kind.PLAYER
      ? store.playerName(e.id)
      : (assets?.moverName(e.prop) ?? `#${String(e.prop)}`);
  return {
    kind: e.kind,
    sub: e.sub,
    level: e.level,
    flags: e.flags,
    name,
    guildId: e.kind === Kind.PLAYER ? guild(e.id) : 0,
  };
}

export class MapEngine {
  readonly store = new EntityStore();
  readonly ui: StoreApi<MapUiState>;
  readonly gl: GlLayer;
  readonly extras: EngineExtras = { highlights: [], atlas: [] };
  readonly heat = new HeatGrid(Date.now());
  readonly trails = new TrailSet();
  /** Called after a frame was drawn with the view (the controller sends the subscription from it). */
  onView: ((v: View) => void) | null = null;
  /** The guild id of a player by object id (the roster knows it). */
  guildOf: (objId: number) => number = () => 0;
  /** The guild of the followed or selected player. */
  focusGuild = 0;
  actingObjId = 0;

  readonly #overlay: HTMLCanvasElement;
  readonly #octx: CanvasRenderingContext2D | null;
  readonly #labels: LabelLayer;
  readonly #io: TileIo;
  readonly #now: () => number;
  readonly #raf: (cb: FrameRequestCallback) => number;
  readonly #caf: (id: number) => void;

  #meta: MetaDoc | null = null;
  #cov = new Coverage(null, [1, 1]);
  #cache: TileCache<GpuTile> | null = null;
  #assets: GameAssets | null = null;
  #view: View = { cx: 0, cz: 0, upp: 2, w: 0, h: 0 };
  #limits: [number, number] = [0.25, 64];
  #fly: Fly | null = null;
  #dpr = 1;
  #raf_id = 0;
  #running = false;
  #dirty = true;
  #overlayDirty = true;
  #viewVersion = 0;
  #planKey = '';
  #tileVersion = 0;
  #selected = new Set<number>();
  #followId: number | null = null;
  #hoverId: number | null = null;
  #box: Rect | null = null;
  #ruler: MapUiState['ruler'] = null;
  #colors: OverlayColors = FALLBACK_COLORS;
  #colorsAt = 0;
  #filter: ParsedFilter = { terms: [], errors: [] };
  #filterFn: ((s: FilterSubject) => boolean) | null = null;
  #last: InstanceResult | null = null;
  #lastLabelsAt = 0;
  #lastUiAt = 0;
  #lastTrailAt = 0;
  #lastHeatAt = 0;
  #frameCost: number[] = [];
  #frames = 0;
  #stats: MapStats = { ...EMPTY_STATS };
  readonly #subject: { f: FilterSubject | null } = { f: null };

  constructor(deps: EngineDeps) {
    this.gl = deps.gl;
    this.ui = deps.ui;
    this.#overlay = deps.overlay;
    this.#octx = deps.overlay.getContext('2d');
    this.#labels = new LabelLayer(deps.labelHost);
    this.#io = deps.io;
    this.#now = deps.now ?? (() => performance.now());
    this.#raf = deps.raf ?? ((cb) => requestAnimationFrame(cb));
    this.#caf =
      deps.caf ??
      ((id) => {
        cancelAnimationFrame(id);
      });
    setS3tc(this.gl.s3tc);
    const atlas = buildAtlas();
    if (atlas) this.gl.setAtlas(atlas);
    this.gl.onContext = (lost) => {
      if (lost) return;
      // the context is back: the tiles died with it
      this.#cache?.forget();
      this.#tileVersion += 1;
      this.#dirty = true;
      this.kick();
    };
  }

  // ---- state the controller reads ---------------------------------------------------------------------------------------------------

  get view(): View {
    return this.#view;
  }
  get meta(): MetaDoc | null {
    return this.#meta;
  }
  get selected(): ReadonlySet<number> {
    return this.#selected;
  }
  get followId(): number | null {
    return this.#followId;
  }
  get limits(): readonly [number, number] {
    return this.#limits;
  }
  get stats(): MapStats {
    return this.#stats;
  }
  get lastResult(): InstanceResult | null {
    return this.#last;
  }
  get assets(): GameAssets | null {
    return this.#assets;
  }
  get tileCache(): TileCache<GpuTile> | null {
    return this.#cache;
  }
  get coverage(): Coverage {
    return this.#cov;
  }

  setAssets(assets: GameAssets): void {
    this.#assets = assets;
    // the sheet may now use the game's arrow and pulse pictures
    const atlas = buildAtlas(assets.pictures);
    if (atlas) this.gl.setAtlas(atlas);
    this.kick();
  }

  // ---- the world ---------------------------------------------------------------------------------------------------------------------

  /** Makes `meta` the world on screen: coverage, tile cache and loader, limits. The entities and the view are the controller's to reset. */
  setWorld(meta: MetaDoc | null): void {
    this.#cache?.release();
    this.#cache = null;
    this.#meta = meta;
    this.#planKey = '';
    this.store.clear();
    this.heat.clear(Date.now());
    this.trails.clear();
    if (!meta) {
      this.#cov = new Coverage(null, [1, 1]);
      this.kick();
      return;
    }
    const lands = meta.geometry.lands;
    this.#cov = new Coverage(meta.tiles, lands);
    const tiles = meta.tiles;
    if (tiles) {
      const load = createTileLoader(tiles, meta.id, this.#io, () => this.gl.s3tc);
      this.#cache = new TileCache<GpuTile>({
        capacity: TILE_CAPACITY,
        maxLoading: MAX_TILE_LOADS,
        load,
        dispose: (t) => {
          disposeTile(t);
        },
        onChange: () => {
          this.#tileVersion += 1;
          this.#dirty = true;
          this.kick();
        },
        now: this.#now,
      });
    }
    this.#limits = upLimits(
      meta.geometry.extent_m,
      Math.max(1, this.#view.w),
      Math.max(1, this.#view.h),
    );
    this.#view = { ...this.#view, upp: clampUpp(this.#view.upp, this.#limits) };
    this.#viewChanged();
    this.kick();
  }

  // ---- size and view --------------------------------------------------------------------------------------------------------------------

  resize(w: number, h: number, dpr: number): void {
    const cw = Math.max(1, Math.round(w));
    const ch = Math.max(1, Math.round(h));
    if (cw === this.#view.w && ch === this.#view.h && dpr === this.#dpr) return;
    this.#dpr = dpr;
    this.gl.resize(cw, ch, dpr);
    this.#overlay.width = Math.round(cw * dpr);
    this.#overlay.height = Math.round(ch * dpr);
    this.#overlay.style.width = `${String(cw)}px`;
    this.#overlay.style.height = `${String(ch)}px`;
    this.#view = { ...this.#view, w: cw, h: ch };
    if (this.#meta) {
      this.#limits = upLimits(this.#meta.geometry.extent_m, cw, ch);
      this.#view = { ...this.#view, upp: clampUpp(this.#view.upp, this.#limits) };
    }
    this.#viewChanged();
    this.kick();
  }

  /** Sets the view (no flight). The scale is clamped; the centre may leave the world by a quarter of the viewport. */
  setView(v: Partial<Pick<View, 'cx' | 'cz' | 'upp'>>): void {
    this.#fly = null;
    let next: View = { ...this.#view, ...v };
    next = { ...next, upp: clampUpp(next.upp, this.#limits) };
    if (this.#meta) next = clampCentre(next, this.#meta.geometry.extent_m);
    this.#view = next;
    this.#viewChanged();
    this.kick();
  }

  /** Flies to a world point, optionally at a new scale (350 ms ease). */
  flyTo(x: number, z: number, upp?: number): void {
    const target: View = {
      ...this.#view,
      cx: x,
      cz: z,
      upp: clampUpp(upp ?? this.#view.upp, this.#limits),
    };
    this.#fly = startFly(this.#view, target, this.#now());
    this.kick();
  }

  panBy(dx: number, dy: number): void {
    this.setView(panBy(this.#view, dx, dy));
  }

  zoomAt(factor: number, sx: number, sy: number): void {
    this.#fly = null;
    const v = zoomAt(this.#view, factor, sx, sy, this.#limits);
    this.#view = this.#meta ? clampCentre(v, this.#meta.geometry.extent_m) : v;
    this.#viewChanged();
    this.kick();
  }

  screenToWorld(sx: number, sy: number): { x: number; z: number } {
    return screenToWorld(this.#view, sx, sy);
  }

  #viewChanged(): void {
    this.#viewVersion += 1;
    this.#overlayDirty = true;
    this.#dirty = true;
  }

  // ---- selection, follow, hover, tools ----------------------------------------------------------------------------------------------------

  setSelection(ids: Iterable<number>): void {
    this.#selected = new Set(ids);
    this.#dirty = true;
    this.#overlayDirty = true;
    this.#updateFocus();
    this.kick();
  }

  setFollow(id: number | null): void {
    this.#followId = id;
    this.#updateFocus();
    this.#dirty = true;
    this.#overlayDirty = true;
    this.kick();
  }

  #updateFocus(): void {
    const id = this.#followId ?? [...this.#selected][0] ?? null;
    this.focusGuild = id === null ? 0 : this.guildOf(id);
  }

  setHover(id: number | null): void {
    if (this.#hoverId === id) return;
    this.#hoverId = id;
    this.#dirty = true;
    this.kick();
  }

  setBox(rect: Rect | null): void {
    this.#box = rect;
    this.#overlayDirty = true;
    this.kick();
  }

  setRuler(r: MapUiState['ruler']): void {
    this.#ruler = r;
    this.#overlayDirty = true;
    this.kick();
  }

  setHighlights(h: readonly Highlight[]): void {
    this.extras.highlights = h;
    this.#overlayDirty = true;
    this.kick();
  }

  setAtlasImages(a: readonly AtlasImage[]): void {
    this.extras.atlas = a;
    this.#overlayDirty = true;
    this.kick();
  }

  /** Re-reads the theme colours (the console's theme changed). */
  themeChanged(): void {
    this.#colorsAt = 0;
    this.#overlayDirty = true;
    this.#dirty = true;
    this.kick();
  }

  /** Something changed: redraw the entities, and the vector overlays too unless `overlays` is `false` (a frame of entities does not move them). */
  markDirty(overlays = true): void {
    this.#dirty = true;
    if (overlays) this.#overlayDirty = true;
    this.kick();
  }

  // ---- filters ------------------------------------------------------------------------------------------------------------------------

  setFilterText(text: string): ParsedFilter {
    this.#filter = parseFilter(text);
    this.#filterFn = isActive(this.#filter) ? compileFilter(this.#filter) : null;
    this.#dirty = true;
    this.kick();
    return this.#filter;
  }

  /** `false` for the entities the Layers tab hides (kinds off, rank and state filters). */
  readonly visible = (e: Ent): boolean => {
    const ui = this.ui.getState();
    switch (e.kind) {
      case Kind.PLAYER:
        return ui.kinds.players;
      case Kind.MONSTER: {
        if (!ui.kinds.monsters) return false;
        const f = ui.monsterFilter;
        const rankOk =
          e.sub === Rank.CAPTAIN
            ? f.captain
            : isBossRank(e.sub) || (e.flags & Flag.BOSS) !== 0
              ? f.boss
              : f.normal;
        if (!rankOk) return false;
        const asleep = (e.flags & Flag.ASLEEP) !== 0;
        return asleep ? f.asleep : f.awake;
      }
      case Kind.NPC:
        return ui.kinds.npcs;
      case Kind.PET:
        return ui.kinds.pets;
      case Kind.ITEM:
        return ui.kinds.items;
      default:
        return ui.kinds.ctrls;
    }
  };

  // ---- picking --------------------------------------------------------------------------------------------------------------------------

  /** The entity under the CSS pixel (sx, sy), or `null`. */
  pickAtScreen(sx: number, sy: number): Ent | null {
    const w = screenToWorld(this.#view, sx, sy);
    return pickAt(this.store, w.x, w.z, {
      upp: this.#view.upp,
      now: this.#now(),
      visible: this.visible,
    });
  }

  /** The entities inside a world rectangle. */
  pickInBox(rect: Rect): Ent[] {
    return pickBox(this.store, rect, {
      upp: this.#view.upp,
      now: this.#now(),
      visible: this.visible,
    });
  }

  /** The display name of an entity (players by name, others by prop). */
  nameOf(e: Ent): string {
    if (e.kind === Kind.PLAYER) return this.store.playerName(e.id);
    const n = this.#assets?.moverName(e.prop);
    if (n) return n;
    return e.kind === Kind.ITEM ? `Item #${String(e.prop)}` : `#${String(e.prop)}`;
  }

  // ---- the loop -------------------------------------------------------------------------------------------------------------------------

  start(): void {
    this.#running = true;
    this.#dirty = true;
    this.#overlayDirty = true;
    this.kick();
  }

  stop(): void {
    this.#running = false;
    if (this.#raf_id !== 0) this.#caf(this.#raf_id);
    this.#raf_id = 0;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  /** Asks for a frame (no-op while one is already scheduled or the engine is stopped). */
  kick(): void {
    if (!this.#running || this.#raf_id !== 0) return;
    this.#raf_id = this.#raf(() => {
      this.#raf_id = 0;
      this.frame();
    });
  }

  /** Draws one frame now (also what the probe calls). Schedules the next one while something still moves. */
  frame(): void {
    if (!this.#running || this.gl.contextLost) return;
    const t0 = performance.now();
    const now = this.#now();
    if (this.#colorsAt === 0 || now - this.#colorsAt > 3000) {
      this.#colors = readColors();
      this.#colorsAt = now;
      this.gl.setClearColor(this.#colors.void);
      this.#dirty = true;
    }
    let moving = false;
    if (this.#fly) {
      const s = sampleFly(this.#fly, now);
      this.#view = this.#meta ? clampCentre(s.view, this.#meta.geometry.extent_m) : s.view;
      if (s.done) this.#fly = null;
      this.#viewChanged();
      moving = !s.done;
    }
    const f = this.#followId === null ? undefined : this.store.byId.get(this.#followId);
    if (f) {
      this.store.place(f, now);
      if (f.x !== this.#view.cx || f.z !== this.#view.cz) {
        this.#view = { ...this.#view, cx: f.x, cz: f.z };
        this.#viewChanged();
      }
    }
    const sliding = this.store.sliding(now);
    const pulsing = this.#selected.size > 0 || this.#followId !== null;
    if (sliding || pulsing) this.#dirty = true;
    if (this.#dirty || this.#viewVersion !== this.#drawnView) this.#drawGl(now);
    if (this.#overlayDirty) this.#drawOverlay(now);
    this.#sample(now);
    this.#publish(now);
    if (this.#notifiedView !== this.#viewVersion) {
      this.#notifiedView = this.#viewVersion;
      this.onView?.(this.#view);
    }
    this.#frames += 1;
    const cost = performance.now() - t0;
    this.#frameCost.push(cost);
    if (this.#frameCost.length > 60) this.#frameCost.shift();
    this.#dirty = false;
    if (moving || sliding) this.kick();
    else if (pulsing) this.#kickLater(100);
  }

  #notifiedView = -1;
  #timer: ReturnType<typeof setTimeout> | null = null;

  /** The next frame in `ms` (the selection pulse and nothing else moves: ten frames a second are plenty). */
  #kickLater(ms: number): void {
    if (this.#timer !== null || !this.#running) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      this.#dirty = true;
      this.kick();
    }, ms);
  }

  #drawnView = -1;

  #drawGl(now: number): void {
    const v = this.#view;
    if (v.w <= 0 || v.h <= 0) return;
    this.#drawnView = this.#viewVersion;
    this.gl.setView(v);
    this.#planTiles();
    // entities
    const ui = this.ui.getState();
    const dim = this.#filterFn;
    const subject = this.#subject;
    const dimFn =
      dim === null
        ? null
        : (e: Ent): boolean => {
            subject.f = subjectOf(e, this.store, this.#assets, this.guildOf);
            return !dim(subject.f);
          };
    const style: StyleContext = {
      focusGuild: this.focusGuild,
      guildOf: this.guildOf,
      actingObjId: this.actingObjId,
    };
    const { result } = buildInstances(
      {
        store: this.store,
        view: v,
        now,
        visible: this.visible,
        dim: dimFn,
        style,
        selected: this.#selected,
        followId: this.#followId,
        hoverId: this.#hoverId,
        showBins: ui.kinds.monsters,
        dimBins: dim !== null,
      },
      (n) => this.gl.ensureCapacity(n),
    );
    this.gl.setInstanceCount(result.count);
    this.gl.render();
    this.#last = result;
    this.#placeLabels(now, result);
  }

  /** The tile plan: recomputed when the view or the set of loaded tiles changed. */
  #planTiles(): void {
    const cache = this.#cache;
    const meta = this.#meta;
    if (!cache || !meta?.tiles) {
      this.gl.setTiles([]);
      return;
    }
    const L = landMeters(meta.geometry.mpu);
    const v = this.#view;
    const level = chooseLevel(v.upp, L, this.#cov.maxLevel);
    const key = `${String(this.#viewVersion)}|${String(this.#tileVersion)}|${String(level)}`;
    if (key === this.#planKey) return;
    this.#planKey = key;
    const view = viewRect(v, 0);
    const span = L * 2 ** level;
    const margin: Rect = [view[0] - span, view[1] - span, view[2] + span, view[3] + span];
    const plan = planTiles(view, margin, L, level, this.#cov, (id) => cache.isReady(id));
    cache.request(plan.wanted, plan.keep);
    const draws: TileDraw[] = [];
    for (const id of plan.draw) {
      const tile = cache.get(id);
      if (!tile) continue;
      draws.push({ tile, rect: tileRect(L, id), order: this.#cov.maxLevel - id.level });
    }
    this.gl.setTiles(draws);
  }

  // ---- labels -------------------------------------------------------------------------------------------------------------------------

  #labelView = -1;

  #placeLabels(now: number, r: InstanceResult): void {
    // the labels follow the view at once; while only the entities slide they are refreshed ten times a second
    if (this.#labelView === this.#viewVersion && now - this.#lastLabelsAt < 100) return;
    this.#labelView = this.#viewVersion;
    this.#lastLabelsAt = now;
    const cands: LabelCandidate[] = [];
    const state = {
      selected: this.#selected,
      followId: this.#followId,
      actingObjId: this.actingObjId,
      close: this.#view.upp <= 1.5,
    };
    for (const e of r.inCanvas) {
      const role = labelRole(e, state);
      if (role) {
        cands.push({
          id: e.id,
          text: this.#labelText(e),
          x: e.x,
          z: e.z,
          priority: role.priority,
          always: role.always,
          cls: role.cls,
        });
      }
      if (cands.length >= 800) break;
    }
    this.#labels.update(selectLabels(cands, this.#view));
  }

  #labelText(e: Ent): string {
    if (e.kind === Kind.PLAYER) return `${this.store.playerName(e.id)} Lv${String(e.level)}`;
    const name = this.nameOf(e);
    if (e.kind === Kind.MONSTER) return `${name} Lv${String(e.level)}`;
    if (e.kind === Kind.ITEM) return e.level > 1 ? `${name} ×${String(e.level)}` : name;
    return name;
  }

  // ---- the overlay -------------------------------------------------------------------------------------------------------------------

  #drawOverlay(now: number): void {
    const ctx = this.#octx;
    const meta = this.#meta;
    if (!ctx || !meta) {
      ctx?.clearRect(0, 0, this.#overlay.width, this.#overlay.height);
      this.#overlayDirty = false;
      return;
    }
    ctx.setTransform(this.#dpr, 0, 0, this.#dpr, 0, 0);
    const ui = this.ui.getState();
    drawOverlays(ctx, this.#view, {
      meta,
      cov: this.#cov,
      toggles: ui.toggles,
      colors: this.#colors,
      heat: ui.toggles.heatmap ? this.heat : null,
      trails: ui.toggles.trails ? this.trails.all() : [],
      // the artworks are Madrigal's: their rectangles mean nothing in another world
      atlas: meta.id === DEFAULT_WORLD ? this.extras.atlas : [],
      highlights: this.extras.highlights,
      ruler: this.#ruler,
      box: this.#box,
      now,
    });
    this.#overlayDirty = false;
  }

  /** Samples the trails (twice a second) and the heatmap (once a second) from the frames the map already has. */
  #sample(now: number): void {
    const ui = this.ui.getState();
    if (now - this.#lastTrailAt >= 500) {
      this.#lastTrailAt = now;
      const ids = new Set<number>(this.#selected);
      if (this.#followId !== null) ids.add(this.#followId);
      this.trails.keepOnly(ids);
      let added = false;
      for (const id of ids) {
        const e = this.store.byId.get(id);
        if (!e) continue;
        this.store.place(e, now);
        if (this.trails.get(id).push(now, e.x, e.z)) added = true;
      }
      if (added && ui.toggles.trails) this.#overlayDirty = true;
    }
    if (ui.toggles.heatmap && now - this.#lastHeatAt >= 1000) {
      const dt = this.#lastHeatAt === 0 ? 1 : (now - this.#lastHeatAt) / 1000;
      this.#lastHeatAt = now;
      for (const e of this.store.byId.values()) {
        if (e.kind === Kind.PLAYER) this.heat.add(e.x1, e.z1, dt);
      }
      this.#overlayDirty = true;
    }
  }

  /** The counters for the tests every frame, the interface state at 4 Hz. */
  #publish(now: number): void {
    const r = this.#last;
    const cache = this.#cache;
    const s = this.#stats;
    s.instances = r?.count ?? 0;
    s.drawn = r?.drawn ?? 0;
    s.labels = this.#labelsCount();
    s.tiles = cache?.size ?? 0;
    s.tilesRequested = cache?.stats.requested ?? 0;
    s.tilesLoading = cache?.loading ?? 0;
    s.entities = this.store.size;
    s.bins = r?.bins ?? 0;
    s.tier = this.store.tier;
    s.frames = this.#frames;
    s.frameMs =
      this.#frameCost.length === 0
        ? 0
        : this.#frameCost.reduce((a, b) => a + b, 0) / this.#frameCost.length;
    s.world = this.ui.getState().worldId;
    s.layer = this.ui.getState().layer;
    s.upp = this.#view.upp;
    s.cx = this.#view.cx;
    s.cz = this.#view.cz;
    s.width = this.#view.w;
    s.height = this.#view.h;
    if (now - this.#lastUiAt < 250) return;
    this.#lastUiAt = now;
    const v = this.#view;
    const prev = this.ui.getState();
    const counts = r?.counts ?? prev.counts;
    const monsters = counts.monsters + (r?.binMonsters ?? 0);
    this.ui.setState({
      view: { cx: v.cx, cz: v.cz, upp: v.upp, w: v.w, h: v.h },
      counts: { ...counts, monsters },
      held: this.store.size,
      tier: this.store.tier,
      degraded: this.store.degraded,
    });
  }

  #labelsCount(): number {
    return this.#labels.count;
  }

  // ---- teardown -------------------------------------------------------------------------------------------------------------------------

  /** Releases the GPU textures (the map was hidden for two minutes). The engine can be used again: tiles load again. */
  releaseTextures(): void {
    this.#cache?.release();
    this.#planKey = '';
    this.#tileVersion += 1;
    this.gl.setTiles([]);
  }

  dispose(): void {
    this.stop();
    this.#cache?.release();
    this.#labels.dispose();
    this.gl.dispose();
  }

  /** The tile at `id` is on the GPU (tests). */
  hasTile(id: TileId): boolean {
    return this.#cache?.isReady(id) ?? false;
  }

  /** The margin around the canvas in which entities are still drawn. */
  static readonly MARGIN_PX = DRAW_MARGIN_PX;
}
