// The `navigator` feature (Plan/45 §7.6, §7.7, §22 navigator marks; ref/99 §4.4, §4.5; WP-CFEAT-28, WP-CFEAT-35): the minimap in
// `APP_NAVIGATOR` and the world-map overlay `APP_MAP` with its teleporter mode.
//
//   minimap        the land textures (`<land>.dds`, decoded by the worker), the mover dots by kind and filter, the focus marker, the own mark
//                  and the marks of the party (`SetNaviPointNtf`), the player arrow, the fog of indoor worlds (`scene.ts`, `paint.ts`)
//   input          L-down: the own mark + `SetNaviPointReq` (a focused player, else the whole party) + selecting the dot; L-up on an NPC focus
//                  clears it; R-down clears focus and destination and lists the NPCs in sight; a double click of a game master sends
//                  `/teleport <world> <x> <z>`; a move over a dot or a mark shows its name
//   title          the region the character entered last (`RegionTracker`, 44) - the first line of its title; an untitled region clears it
//   world map      key M toggles it (`TID_GAME_MAPMSG` where there is no map); the menu entry `MMI_TELEPORTER` opens it in teleporter mode with
//                  the destinations of the NPC (`npcs[key].teleports`); a click on an icon sends `Teleporter{key, index}`
//   persistence    the filters and the zoom travel with the window rect (`WndNavigator.serializeRegInfo`)
//
// Other features reach it with `navigatorOf(ctx)`: the model, the world map controller, the last scene (probe, tests).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { inputProvidersOf } from '../../app/inputProviders';
import { isAssetError } from '../../assets/errors';
import { resKey } from '../../assets/source';
import { EntityKind } from '../../game/entities';
import { MMI_TELEPORTER } from '../../gen/defs/neuz';
import { TID_GAME_MAPMSG, TID_GAME_NAV_MARK } from '../../gen/defs/text';
import { TeleporterSchema } from '../../gen/proto/flyff/v1/npc_pb';
import type { InputInstance } from '../../input/input';
import type { ClientRegion } from '../../gen/bundles';
import { ddsToPixelImage } from '../../ui/skin/ddsImage';
import type { NavigatorActions, NavigatorPorts, ViewSource } from '../../ui/windows/nav/ports';
import { Prio } from '../../workers/priority';
import { chatOf } from '../chat/chatModule';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { menuOf } from '../npc/menuModule';
import { partyOf } from '../party/partyModule';
import { type ContinentPoly } from './continent';
import { type NavEnv } from './dots';
import { gmTeleportTarget, viewToWorld, type ViewSize } from './geometry';
import { DestinationState } from './destination';
import { NavigatorModel } from './model';
import { paintScene } from './paint';
import {
  AUTH_GAMEMASTER,
  cameraHeading,
  isInstanceWorld,
  madrigalPolys,
  positionOf,
  type QuestEmoticonSource,
  readMovers,
  selfPosition,
  worldOf,
} from './runtime';
import {
  buildScene,
  dotAt,
  dotRect,
  markAt,
  type MarkInput,
  type NavInput,
  type NavScene,
  pickMover,
} from './scene';
import { NavSprites } from './sprites';
import { MinimapTiles, type TilePixels } from './tiles';
import { WorldMapController } from './worldMapController';

/** The public face of the navigator feature of a context. */
export interface NavigatorFeature {
  readonly model: NavigatorModel;
  readonly tiles: MinimapTiles;
  /** The pictures of the minimap (`null` until the UI exists). */
  sprites(): NavSprites | null;
  readonly worldMap: WorldMapController;
  readonly actions: NavigatorActions;
  /** The inputs of a frame for a view of `size` (or `null` outside a world); tests and the probe read them. */
  input(size: ViewSize, now: number): NavInput | null;
  /** The scene of the last paint, `null` before the first. */
  lastScene(): NavScene | null;
  /** The region entered (`SetRegionName`): the first line of the title, an untitled region clears the name. */
  onRegionEntered(region: Pick<ClientRegion, 'title'>): void;
  /** The destination arrow (`m_vDestinationArrow`; `destination.ts`): the minimap sprite, the world map and the 3-D arrow of render-audio-m7 read it. */
  readonly destination: DestinationState;
  /**
   * Installs the quest marks of the NPCs (`m_nQuestEmoticonIndex`, 45 WP-CFEAT-42): the minimap draws frame `20 + index` for an NPC with an index of 0..3. `null` removes it: every NPC then has
   * -1 (no quest dot, the shipped quest-less data).
   */
  setQuestEmoticonSource(fn: QuestEmoticonSource | null): void;
}

const features = new WeakMap<ClientContext, NavigatorFeature>();

/** The navigator feature of a context; `undefined` before the module registered. */
export function navigatorOf(ctx: ClientContext): NavigatorFeature | undefined {
  return features.get(ctx);
}

export const navigatorModule: FeatureModule = {
  id: 'map',
  milestone: 4,
  register(fc) {
    registerNavigator(fc);
  },
};

/** The title of the navigator for a region: its first non-empty title line, `''` for an untitled region. */
export function regionNameOf(title: string | null | undefined): string {
  if (title === null || title === undefined || title === '') return '';
  const first = title.split(/\r?\n/)[0] ?? '';
  return first;
}

function registerNavigator(fc: FeatureContext): NavigatorFeature {
  const ctx = fc.app;
  const model = new NavigatorModel();
  const destination = new DestinationState();
  let emoticonSource: QuestEmoticonSource | null = null;
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);

  // ---- the land textures -----------------------------------------------------------------------------------------------------

  const tiles = new MinimapTiles({
    io: {
      fetch: async (path) => {
        const key = resKey(path);
        if (!key) return null;
        try {
          return await ctx.assets.fetchBytes(key, Prio.Visible);
        } catch (e) {
          if (isAssetError(e) && e.missing) return null;
          throw e;
        }
      },
      decode: async (bytes): Promise<TilePixels | null> => {
        const pool = ctx.workers;
        if (!pool) return null;
        const r = await pool.submit({ kind: 'dds_bc', bytes, prio: Prio.Visible }).promise;
        if (!r.blob) return null;
        const img = ddsToPixelImage({ meta: r.meta, blob: new Uint8Array(r.blob) });
        return img ? { width: img.width, height: img.height, data: img.data } : null;
      },
    },
    toImage: (p) => {
      if (typeof document === 'undefined') return null;
      const c = document.createElement('canvas');
      c.width = p.width;
      c.height = p.height;
      const g = c.getContext('2d');
      if (!g) return null;
      g.putImageData(
        new ImageData(p.data as Uint8ClampedArray<ArrayBuffer>, p.width, p.height),
        0,
        0,
      );
      return c;
    },
  });

  let sprites: NavSprites | null = null;
  const spritesOf = (): NavSprites | null => {
    if (sprites) return sprites;
    const ui = fullUi(ctx);
    if (!ui) return null;
    sprites = new NavSprites({ atlases: ui.atlases, images: ui.images });
    void sprites.load();
    return sprites;
  };

  // ---- the inputs of a frame -------------------------------------------------------------------------------------------------

  const polysCache = new WeakMap<object, readonly ContinentPoly[]>();
  const polys = (): readonly ContinentPoly[] => {
    const key = ctx.data?.worldFolder('wdmadrigal');
    if (!key) return [];
    let p = polysCache.get(key);
    if (!p) {
      p = madrigalPolys(ctx);
      polysCache.set(key, p);
    }
    return p;
  };

  const targeting = (): InputInstance['core']['targeting'] | null =>
    (ctx.input as Partial<InputInstance>).core?.targeting ?? null;
  const commands = (): InputInstance['core']['commands'] | null =>
    (ctx.input as Partial<InputInstance>).core?.commands ?? null;

  const navEnv = (): NavEnv => {
    const self = ctx.stores.selfStore.getState();
    const party = partyOf(ctx);
    const members = party?.store.getState().members ?? [];
    return {
      filters: model.filters,
      selfId: ctx.entities.self()?.objId ?? 0,
      selfGuildId: self.guildId,
      murdererId: model.murdererId,
      isPartyMember: (idPlayer) => members.some((m) => m.id === idPlayer),
      indicator: ctx.features.features.navigator_indicator,
    };
  };

  const marks = (now: number): MarkInput[] => {
    const out: MarkInput[] = [];
    const own = model.ownMark(now);
    if (own) {
      out.push({ x: own.x, z: own.z, name: ctx.stores.selfStore.getState().name, self: true });
    }
    for (const m of partyOf(ctx)?.marks() ?? []) {
      out.push({ x: m.pos[0], z: m.pos[2], name: m.name, self: false });
    }
    return out;
  };

  const input = (size: ViewSize, now: number): NavInput | null => {
    const world = worldOf(ctx);
    const pos = selfPosition(ctx);
    if (!world || !pos) return null;
    const fogged = world.indoor || isInstanceWorld(world.worldId);
    const focusId = targeting()?.focus ?? null;
    const focus = focusId === null ? null : positionOf(ctx, focusId);
    return {
      size: model.zoom,
      view: size,
      px: pos.x,
      pz: pos.z,
      heading: cameraHeading(ctx, pos.x, pos.z),
      lands: world.lands,
      fogged,
      movers: fogged ? [] : readMovers(ctx, [], emoticonSource),
      env: navEnv(),
      focus,
      marks: marks(now),
      destination: destination.get(),
      nowMs: now,
    };
  };

  let last: NavScene | null = null;

  // ---- the actions of the window ----------------------------------------------------------------------------------------------

  const actions: NavigatorActions = {
    zoom(direction) {
      model.zoomBy(direction);
    },
    toggleFilter(id) {
      model.toggleFilter(id);
    },
    lButtonDown(pt, size) {
      const now = ctx.now();
      const world = worldOf(ctx);
      const pos = selfPosition(ctx);
      if (!world || !pos) return;
      // the own mark and its send: a focused player gets it with the sender, else the whole party
      const at = viewToWorld(pt.x, pt.y, pos.x, pos.z, model.zoom, size);
      model.setOwnMark(at.x, pos.y, at.z, now);
      partyOf(ctx)?.naviClick([at.x, pos.y, at.z]);
      // then the dot under the pointer is selected
      const hit = pickMover(
        readMovers(ctx, [], emoticonSource),
        ctx.entities.self()?.objId ?? 0,
        pos.x,
        pos.z,
        model.zoom,
        size,
        pt.x,
        pt.y,
      );
      if (hit) targeting()?.setFocus(hit.id);
    },
    lButtonUp() {
      const t = targeting();
      const focus = t?.focus ?? null;
      if (focus === null) return;
      const rec = ctx.entities.get(focus);
      // `IsNPC()`: every mover that is not a player (monsters too)
      if (rec && !rec.removed && rec.kind === EntityKind.Npc) {
        t?.setFocus(null);
        commands()?.clearDest();
      }
    },
    rButtonDown() {
      targeting()?.setFocus(null);
      commands()?.clearDest();
      // `__FIND_OBJ_INSIGHT`: the NPCs with a character in sight
      const out: { id: number; name: string }[] = [];
      for (const m of readMovers(ctx)) {
        if (m.isPlayer) continue;
        const rec = ctx.entities.get(m.id);
        const key =
          rec?.kind === EntityKind.Npc
            ? (rec.spawn as { npc?: { characterKey?: string } }).npc?.characterKey
            : '';
        if (key) out.push({ id: m.id, name: m.name });
      }
      return out;
    },
    doubleClick(pt, size) {
      const self = ctx.stores.selfStore.getState();
      if (self.authority < AUTH_GAMEMASTER) return;
      const pos = selfPosition(ctx);
      const world = worldOf(ctx);
      if (!pos || !world) return;
      const at = gmTeleportTarget(pt.x, pt.y, pos.x, pos.z, model.zoom, size);
      chatOf(ctx)?.submit(
        `/teleport ${String(world.worldId)} ${at.x.toFixed(6)} ${at.z.toFixed(6)}`,
      );
    },
    hover(pt) {
      if (!last) return null;
      const m = markAt(last, pt.x, pt.y);
      if (m) {
        return {
          text: `${text(TID_GAME_NAV_MARK)}${m.name}`,
          rect: { l: m.x - 8, t: m.y - 8, r: m.x + 8, b: m.y + 8 },
        };
      }
      const d = dotAt(last, pt.x, pt.y);
      if (d) return { text: d.dot.name, rect: dotRect(d) };
      return null;
    },
    pickNpc(id) {
      targeting()?.setFocus(id);
      // V15 (`__IMPROVE_QUEST_INTERFACE`, `CWndNavigator::OnCommand` of the NPC popup): the picked NPC's position is the destination, also without quest data
      const at = positionOf(ctx, id);
      if (at) destination.set(at.x, at.z);
    },
    restore(filters, zoom) {
      model.restore(filters, zoom);
    },
  };

  // ---- the ports of the window --------------------------------------------------------------------------------------------------

  const view: ViewSource<ReturnType<NavigatorModel['view']>> = {
    get: () =>
      model.view(worldOf(ctx)?.indoor === true || isInstanceWorld(worldOf(ctx)?.worldId ?? 0)),
    subscribe: model.subscribe,
  };

  const ports: NavigatorPorts = {
    view,
    actions,
    paint(g, size, now) {
      const s = spritesOf();
      const i = input(size, now);
      if (!s || !i) {
        last = null;
        return;
      }
      const world = worldOf(ctx);
      if (world) tiles.setWorld(world.folder);
      const scene = buildScene(i);
      last = scene;
      paintScene(g, scene, {
        tiles,
        object: (f) => s.object(f),
        mark: (f) => s.mark(f),
        arrow: s.arrow,
        fog: s.fog,
        destination: s.destination,
      });
    },
  };

  // ---- the world map ----------------------------------------------------------------------------------------------------------------

  const worldMap = new WorldMapController({
    maps: () => ctx.data?.bundle('worlds')?.world_maps ?? [],
    polys,
    viewport: () => {
      const vp = fullUi(ctx)?.wm.viewport;
      return vp ? { w: vp.r - vp.l, h: vp.b - vp.t } : { w: 1280, h: 720 };
    },
    worldId: () => ctx.stores.worldStore.getState().worldId,
    self: () => {
      const p = selfPosition(ctx);
      return p
        ? {
            x: p.x,
            z: p.z,
            heading: cameraHeading(ctx, p.x, p.z),
            name: ctx.stores.selfStore.getState().name,
          }
        : null;
    },
    members: () => {
      const party = partyOf(ctx);
      if (!party) return [];
      const selfId = ctx.stores.selfStore.getState().idPlayer;
      const out: { name: string; x: number; z: number }[] = [];
      for (const m of party.store.getState().members) {
        if (m.id === selfId || m.offline) continue;
        const pos = party.memberPosition(m.id);
        if (pos) out.push({ name: party.playerData.nameOf(m.id), x: pos[0], z: pos[2] });
      }
      return out;
    },
    buttonSprite: (frame) => {
      const dto = ctx.bundles.get('ui-atlases')?.atlases['theme/texmapbutton.inc'];
      const s = dto?.sprites[frame];
      return dto && s ? { path: dto.texture, x: s.x, y: s.y, w: s.w, h: s.h } : null;
    },
    sendTeleporter: (key, index) => {
      // legacy-send: SendTeleporterReq
      ctx.net.sender.send({
        case: 'teleporter',
        value: create(TeleporterSchema, { characterKey: key, index }),
      });
    },
    click: () => {
      fullUi(ctx)?.host.sound?.click();
    },
    noMapMessage: () => {
      sysChat(fc.texts, fc.chat, TID_GAME_MAPMSG);
    },
    now: () => ctx.now(),
    destination: () => destination.get(),
  });

  // 10 Hz while the overlay is shown: the arrows move
  ctx.publisher.register('worldmap', 10, () => {
    worldMap.refresh();
  });

  // the input layer: Esc closes, a click on the 3-D view is routed to the overlay (UI pixels = CSS pixels / the UI scale)
  inputProvidersOf(ctx).worldMap = {
    shown: () => worldMap.shown,
    hide: () => {
      worldMap.close();
    },
    click: (x, y) => {
      const scale = fullUi(ctx)?.scaler.scale ?? 1;
      worldMap.click(x / scale, y / scale);
    },
  };

  // `MMI_TELEPORTER`: the whole world map with the destinations of the NPC (`ProcessingTeleporter`)
  menuOf(ctx)?.registerAction(MMI_TELEPORTER, ({ npcKey }) => {
    if (!npcKey) return;
    const row = ctx.data?.npc(npcKey);
    if (!row || row.teleports.length === 0) return;
    worldMap.openTeleporter(row.key, row.teleports);
  });

  // ---- the title: the region entered last -----------------------------------------------------------------------------------------

  const onRegionEntered = (region: Pick<ClientRegion, 'title'>): void => {
    model.setRegionName(regionNameOf(region.title));
  };
  let hooked = false;
  const hookRegions = (): void => {
    if (hooked) return;
    const tracker = (
      ctx.audio as { regions?: { onEntered(fn: (r: ClientRegion) => void): () => void } | null }
    ).regions;
    if (!tracker) return;
    hooked = true;
    tracker.onEntered(onRegionEntered);
  };
  // a world change clears the destination arrow (`DPClient.cpp:2467-2471`, V15)
  let lastWorldId = ctx.stores.worldStore.getState().worldId;
  ctx.stores.worldStore.subscribe((s) => {
    if (s.worldId === lastWorldId) return;
    lastWorldId = s.worldId;
    destination.clear();
  });
  // without the audio tracker (no audio engine): the world store's region title at 4 Hz
  let lastTitle: string | null = null;
  ctx.stores.worldStore.subscribe((s) => {
    if (hooked) return;
    if (s.region.title === lastTitle) return;
    lastTitle = s.region.title;
    onRegionEntered({ title: s.region.title });
  });

  // ---- the UI ports ----------------------------------------------------------------------------------------------------------------

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    hookRegions();
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.nav.setPorts({ navigator: ports, worldMap: worldMap.ports() });
  };
  ctx.tickSystems.register('navigator.ui', ensureUi);
  ctx.machine.subscribe((screen, previous) => {
    ensureUi();
    // the world is left: the mark, the title and the overlay go
    if (previous.s === 'InGame' && screen.s !== 'InGame' && screen.s !== 'Reconnecting') {
      model.reset();
      destination.clear();
      worldMap.close();
      tiles.clear();
    }
  });
  ensureUi();

  // ---- the probe ------------------------------------------------------------------------------------------------------------------------

  addProbeMember(ctx, 'navigator', () => {
    const ui = fullUi(ctx);
    const w = ui?.nav.navigator() ?? null;
    const scale = ui?.scaler.scale ?? 1;
    const css = (r: { l: number; t: number; r: number; b: number } | null) =>
      r
        ? {
            x: Math.round(((r.l + r.r) / 2) * scale),
            y: Math.round(((r.t + r.b) / 2) * scale),
            l: Math.round(r.l * scale),
            t: Math.round(r.t * scale),
            w: Math.round((r.r - r.l) * scale),
            h: Math.round((r.b - r.t) * scale),
          }
        : null;
    const c = w?.getScreenClientRect() ?? null;
    const button = (
      b: { getScreenRect(): { l: number; t: number; r: number; b: number } } | null | undefined,
    ) => css(b ? b.getScreenRect() : null);
    return {
      open: w !== null,
      zoom: model.zoom,
      filters: model.filters,
      destination: destination.get(),
      regionName: model.regionName,
      title: w?.getTitle() ?? null,
      size: w?.mapSize ?? null,
      map: css(c),
      place: button(w?.placeButton),
      placeMenu: w?.placeMenu?.isVisible()
        ? w.placeMenu.items.map((it, id) => ({
            id,
            ...(css(it.getScreenRect()) ?? { x: 0, y: 0 }),
          }))
        : null,
      zoomIn: button(w?.zoomInButton),
      zoomOut: button(w?.zoomOutButton),
      tiles: { ...tiles.stats, size: tiles.size, busy: tiles.busy, folder: tiles.folder },
      scene: last
        ? {
            tiles: last.tiles.map((t) => [t.lx, t.lz, t.x, t.y]),
            fogged: last.fogged,
            dots: last.dots.map((d) => ({
              id: d.dot.id,
              frame: d.dot.frame,
              x: d.x,
              y: d.y,
              name: d.dot.name,
            })),
            // the quest marks (frame 20 + index; drawn after the dots, WP-CFEAT-42)
            quests: last.quests.map((d) => ({
              id: d.dot.id,
              frame: d.dot.frame,
              x: d.x,
              y: d.y,
              name: d.dot.name,
            })),
            marks: last.marks,
            destinationMark: last.destination,
            focus: last.focus,
            heading: last.heading,
          }
        : null,
      frames: w?.map.frames ?? 0,
    };
  });
  addProbeMember(ctx, 'worldMap', () => ({
    shown: worldMap.shown,
    mapNo: worldMap.mapNo,
    teleporter: worldMap.teleporterMode,
    log: [...worldMap.log],
    view: worldMap.view(),
  }));
  // the e2e hooks: the CSS pixel centres of the overlay's icons and buttons, and the teleporter mode of an NPC key (the shipped world
  // places no teleporter NPC; the menu entry `MMI_TELEPORTER` runs the same call)
  addProbeMember(ctx, 'worldMapPoints', () => {
    const scale = fullUi(ctx)?.scaler.scale ?? 1;
    const v = worldMap.view();
    const c = (r: { l: number; t: number; r: number; b: number }) => ({
      x: Math.round(((r.l + r.r) / 2) * scale),
      y: Math.round(((r.t + r.b) / 2) * scale),
    });
    return {
      icons: v.icons.map((i) => ({
        index: i.index,
        ...c({ l: i.x, t: i.y, r: i.x + i.w, b: i.y + i.h }),
      })),
      buttons: v.buttons.map((b, index) => ({ index, ...c(b.rect) })),
      box: v.box,
    };
  });
  addProbeMember(ctx, 'worldMapTeleporter', (key: string) => {
    const row = ctx.data?.npc(key);
    return row ? worldMap.openTeleporter(row.key, row.teleports) : false;
  });

  const feature: NavigatorFeature = {
    model,
    tiles,
    sprites: spritesOf,
    worldMap,
    actions,
    input,
    lastScene: () => last,
    onRegionEntered,
    destination,
    setQuestEmoticonSource: (fn) => {
      emoticonSource = fn;
    },
  };
  features.set(ctx, feature);
  return feature;
}
