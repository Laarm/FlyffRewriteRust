// The in-world overlay of the live game (Plan/41 section 19, WP-REND-14; task wire-overlay): the Canvas2D layer above the
// world canvas that draws the nameplates, HP bars, speech balloons, emoticons, vendor titles and damage numbers.
//
// `render/overlay/` is complete and tested on its own; this host is the step that was left open (its NOTES "Wiring"):
//
//   * the layer is created after `WorldRenderer.init` (`attach`), its textures come from the UI's image cache and atlases
//     (`ui.images` / `ui.atlases`: the same decode path and the same client forms as the windows),
//   * `fx` is the overlay half of `RenderFx` (`balloon` / `emoticon` / `vendorTitle` / `damageNumber`); the render host hands it
//     to `composeFx` through `WorldRendererOptions.fxWrap`, so `ctx.render.fx` and `renderer.fx` are the same object,
//   * `draw` runs after `WorldRenderer.draw` of the same frame: the subjects are the movers the mover batcher drew (entity
//     record + position + head height of the model), the projector is the camera rig of that frame,
//   * the options (`RenderOptions.name*` of the options store, the PK colours of the `tables` bundle, the GM/admin titles and
//     colours and `features.guild_color` / `damage_space` of the config) reach `OverlayRenderer.configure`,
//   * `tick` runs the 60 Hz cosmetic steps (damage numbers), entity removal and world changes clear what floats.
//
// Everything of the overlay chunk comes through the `OverlayChunk` seam of the render chunk (the host never imports `render/`
// values itself: the chunk loads lazily).

import { defineOptionsStore } from '../features/options';
import { fullUi } from '../features/common/module';
import { siegeMarksOf } from '../features/guildmarks';
import { questOf } from '../features/quest/api';
import { WI_WORLD_GUILDWAR } from '../gen/defs/world';
import { RENDER_VIEW } from '../gen/sim/wire';
import type { RenderFx } from '../render/fx';
import type { buildRampLut } from '../render';
import type {
  composeFx,
  createOverlayAssets,
  DrawnMark,
  DrawnQuestMark,
  DrawnText,
  NameColors,
  OverlayFx,
  OverlayLayer,
  OverlayRenderer,
  OverlayStats,
  OverlaySubject,
  OverlayTextureSource,
  OverlayViewer,
  parseArgb,
  projectorOfCamera,
  subjectOf,
} from '../render';
import type { EntityLike } from '../render/overlay';
import type { WorldRenderer } from '../render';
import { MISSING } from '../ui/skin/images';
import type { CameraState } from '../render/types';
import type { ClientContext } from './context';

/** What the host needs of the lazily imported overlay modules (part of `RenderChunk`; a test seam). */
export interface OverlayChunk {
  readonly OverlayLayer: typeof OverlayLayer;
  readonly createOverlayAssets: typeof createOverlayAssets;
  readonly composeFx: typeof composeFx;
  readonly subjectOf: typeof subjectOf;
  readonly projectorOfCamera: typeof projectorOfCamera;
  readonly parseArgb: typeof parseArgb;
  readonly buildRampLut: typeof buildRampLut;
}

export interface OverlayHost {
  /** The overlay half of `RenderFx` (requests before `attach` are dropped: balloons and numbers are cosmetic). */
  readonly fx: OverlayFx;
  /** `fx` composed with the base `RenderFx` of the renderer (what `WorldRendererOptions.fxWrap` returns). */
  compose(base: RenderFx): RenderFx;
  /** Creates the DOM layer once the renderer is initialised. A browser without a 2-D context leaves the overlay off. */
  attach(renderer: WorldRenderer): void;
  /** One frame, after `WorldRenderer.draw` of the same camera. */
  draw(renderer: WorldRenderer, camera: CameraState, nowMs: number): void;
  /** `DuelCount`: the PvP countdown sprite (3, 2, 1, 0 = "fight") for 1.5 s in the middle of the view (WP-REND-25). */
  countdown(n: number): void;
  /** The last overlay frame drew the countdown sprite (its texture is resident). */
  countdownDrawn(): boolean;
  /** `due` cosmetic 60 Hz steps. */
  tick(due: number): void;
  /** The world changed or was left: everything floating goes, the layer follows the visibility of the world canvas. */
  worldChanged(visible: boolean): void;
  /** The layer exists. */
  readonly attached: boolean;
  /** The last frame's counters (`null` before the first draw). */
  readonly stats: OverlayStats | null;
  dispose(): void;
}

/** The head height of a visual without a box (nothing is drawn for such a mover before it has parts): the anchor is its feet. */
const NO_HEAD = 0;

/** `bb.max.y` of the model of the visual (unscaled), the height of the head anchor (Plan/41 19.1). */
function headYOf(visual: { readonly bounds: { readonly max: { readonly y: number } } }): number {
  const y = visual.bounds.max.y;
  return Number.isFinite(y) ? y : NO_HEAD;
}

/** Applies the options-window ramp to a text colour (`0xAARRGGBB`): the overlay is drawn after the device ramp pass. */
function rampColor(lut: Uint8Array, argb: number): number {
  const r = lut[(argb >>> 16) & 255] ?? 0;
  const g = lut[(argb >>> 8) & 255] ?? 0;
  const b = lut[argb & 255] ?? 0;
  return ((argb & 0xff000000) | (r << 16) | (g << 8) | b) >>> 0;
}

/** The guild logo pictures (`icon/icon_cloakslogo01..27.jpg`): the original loaded them with the colour key pure black (`D3DCOLOR_XRGB(0, 0, 0)`). */
const GUILD_LOGO_PATH = /^icon\/icon_cloakslogo\d\d\.jpg$/;

/**
 * The pure black colour key of a logo picture: every pixel that is exactly `(0, 0, 0)` becomes transparent. The pictures are JPEGs, so the noise
 * around the edge leaves most near-black pixels opaque, as it did in the original. A new image; the cached one is untouched.
 */
export function keyBlack<T extends { readonly data: Uint8ClampedArray }>(img: T): T {
  const data = new Uint8ClampedArray(img.data);
  for (let i = 0; i + 3 < data.length; i += 4) {
    if (data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 0) data[i + 3] = 0;
  }
  return { ...img, data };
}

export function createOverlayHost(
  ctx: ClientContext,
  chunk: OverlayChunk,
  canvas: HTMLCanvasElement,
): OverlayHost {
  let layer: OverlayLayer | null = null;
  let renderer: OverlayRenderer | null = null;
  let visible = canvas.style.display !== 'none';
  const offs: (() => void)[] = [];
  const subjects: OverlaySubject[] = [];
  const scratchPos: [number, number, number] = [0, 0, 0];
  let lut: Uint8Array | null = null;
  let lutKey = '';
  let last: OverlayStats | null = null;
  /** CPU time of the last overlay frame and its moving average (ms), for the probe. */
  let frameMs = 0;
  let avgMs = 0;

  const fx: OverlayFx = {
    balloon: (id, b) => {
      renderer?.balloon(id, b);
    },
    emoticon: (id, set, index) => {
      renderer?.emoticon(id, set, index);
    },
    vendorTitle: (id, title) => {
      renderer?.vendorTitle(id, title);
    },
    damageNumber: (pos, value, style) => {
      renderer?.damageNumber(pos, value, style);
    },
  };

  /** The textures through the UI's decode path (the window skin images and sprite atlases). */
  const source: OverlayTextureSource = {
    sprite: async (atlas, index) => {
      const ui = fullUi(ctx);
      return ui ? await ui.atlases.sprite(atlas, index) : undefined;
    },
    image: async (path) => {
      const ui = fullUi(ctx);
      if (!ui) return undefined;
      const img = await ui.images.get(path, 'exact');
      if (img === MISSING) return undefined;
      // the guild logos are black-keyed pictures (the other overlay pictures come keyed from the server)
      return GUILD_LOGO_PATH.test(path) ? keyBlack(img) : img;
    },
  };

  /** The options, the PK colours and the staff names of the frame (re-read when the options store or a bundle changes). */
  const configure = (): void => {
    if (!renderer) return;
    const o = defineOptionsStore(ctx.stores.registry).getState().render;
    const pk = (
      ctx.bundles.get('tables') as
        { pk?: { general_color?: number; ready_color?: number; chao_color?: number } } | undefined
    )?.pk;
    const base = renderer.config.colors;
    const g = ctx.features.gameplay;
    const colors: NameColors = {
      general: pk?.general_color ?? base.general,
      ready: pk?.ready_color ?? base.ready,
      chao: pk?.chao_color ?? base.chao,
      gm: chunk.parseArgb(g.gm_name_color, base.gm),
      admin: chunk.parseArgb(g.admin_name_color, base.admin),
    };
    renderer.configure({
      names: {
        nameSelf: o.nameSelf,
        nameOthers: o.nameOthers,
        nameMonsters: o.nameMonsters,
        hidePets: o.hidePets,
        guildColor: ctx.features.features.guild_color,
      },
      colors,
      titles: { gm: g.gm_title, admin: g.admin_title },
      damageSpace: ctx.features.features.damage_space,
    });
  };

  /** The level and the job TYPE of a player for the master / hero icon: the own ones from the self store, another player's from the mirror. */
  const legendFacts = (
    rec: {
      readonly isSelf: boolean;
      readonly visual?: { readonly level?: number; readonly job?: number };
    },
    own: { readonly level: number; readonly job: number },
  ): { level?: number; jobType?: number } => {
    const v = rec.visual;
    if (!v) return {};
    const level = rec.isSelf ? own.level : v.level;
    const job = rec.isSelf ? own.job : v.job;
    const type = job === undefined ? undefined : ctx.data?.job(job)?.type;
    return {
      ...(level !== undefined ? { level } : {}),
      ...(type !== undefined ? { jobType: type } : {}),
    };
  };

  /** `m_nQuestEmoticonIndex` of an NPC (`questOf(ctx).emoticonOf`); nothing while the quest data is empty (PL-148) or for anything but an NPC. */
  const questFacts = (rec: {
    readonly objId: number;
    readonly visual?: { readonly kind?: string };
  }): { questEmoticon?: number } => {
    if (rec.visual?.kind !== 'npc') return {};
    const quests = questOf(ctx);
    if (quests?.available() !== true) return {};
    const index = quests.emoticonOf(rec.objId);
    return index > 0 ? { questEmoticon: index } : {};
  };

  const host: OverlayHost = {
    fx,
    compose: (base) => chunk.composeFx(base, fx),
    get attached() {
      return layer !== null;
    },
    get stats() {
      return last;
    },
    attach(r) {
      if (layer) return;
      const created = chunk.OverlayLayer.create({
        worldCanvas: canvas,
        assets: chunk.createOverlayAssets(source),
        now: () => ctx.now(),
        dprCap: r.options.dprCap,
        colorMap: (argb) => (lut ? rampColor(lut, argb) : argb),
      });
      if (!created) {
        ctx.log.warn('render', 'the overlay layer could not start: no 2-D canvas context');
        return;
      }
      layer = created;
      renderer = created.renderer;
      // the same stacking and visibility as the world canvas: above it, below the DOM UI, hidden without a world
      const style = created.canvas.style;
      style.position = 'fixed';
      style.zIndex = canvas.style.zIndex || '0';
      style.display = visible ? 'block' : 'none';
      configure();
      offs.push(defineOptionsStore(ctx.stores.registry).subscribe(configure));
      offs.push(
        ctx.bundles.subscribe((name) => {
          if (name === 'tables') configure();
        }),
      );
      // the object is deleted: its balloon, emoticon and vendor title go with it
      offs.push(
        ctx.entities.onRemove((rec) => {
          renderer?.removeFor(rec.objId);
        }),
      );
      if (!ctx.probe.sections().includes('overlay')) {
        ctx.probe.extend('overlay', {
          /** Counters of the last frame. */
          stats: () => (last ? { ...last, ms: frameMs, msAvg: avgMs } : null),
          /** The texts of the last frame with their screen positions (turns the recording on at the first call). */
          texts: (): readonly DrawnText[] => {
            if (renderer) renderer.recordTexts = true;
            return renderer?.drawnTexts ?? [];
          },
          /** The guild mark pictures of the last frame: the icon, logo and best / winner mark with their screen positions (turns the recording on at the first call). */
          marks: (): readonly DrawnMark[] => {
            if (renderer) renderer.recordMarks = true;
            return renderer?.drawnMarks ?? [];
          },
          /** The quest emoticons of the last frame: NPC, index, screen position, pulse scale (turns the recording on at the first call; WP-REND-27). */
          questMarks: (): readonly DrawnQuestMark[] => {
            if (renderer) renderer.recordQuestMarks = true;
            return renderer?.drawnQuestMarks ?? [];
          },
          attached: () => layer !== null,
        });
      }
    },
    draw(r, camera, nowMs) {
      if (!layer || !renderer || !visible) return;
      const win = typeof window === 'object' ? window : null;
      const w = canvas.clientWidth > 0 ? canvas.clientWidth : (win?.innerWidth ?? 0);
      const h = canvas.clientHeight > 0 ? canvas.clientHeight : (win?.innerHeight ?? 0);
      if (w <= 0 || h <= 0) return;
      layer.resize(w, h, win?.devicePixelRatio ?? 1);
      // the colour ramp of the options (gamma, bright, contrast) as the world pass applies it
      const opts = r.options;
      const key = `${String(opts.gamma)}/${String(opts.bright)}/${String(opts.contrast)}`;
      if (key !== lutKey) {
        lutKey = key;
        lut = chunk.buildRampLut(opts.gamma, opts.bright, opts.contrast);
      }
      const t0 = performance.now();
      const self = ctx.entities.self();
      subjects.length = 0;
      const own = ctx.stores.selfStore.getState();
      r.movers?.forEachDrawn((e, visual) => {
        const rec = ctx.entities.get(e.id);
        if (!rec) return;
        // the own record keeps the HP of the spawn; the live pool (and its maximum, once the rules publish it) is the store's
        const subject: EntityLike = rec.isSelf ? { ...rec, hp: own.hp, maxHp: own.maxHp } : rec;
        // a pet's anchor is its smoothed screen position, everything else follows the logical position (KEEP-QUIRK)
        const s = chunk.subjectOf(
          subject,
          {
            pos: e.pet ? e.drawPos : e.pos,
            headY: headYOf(visual),
            culled: false,
            ...legendFacts(rec, own),
            // the quest emoticon of an NPC; nothing at all while the quest data is empty (PL-148: `emoticonOf` answers -1 then too)
            ...questFacts(rec),
          },
          camera.pos,
        );
        if (s) subjects.push(s);
      });
      const guild = self?.visual?.guild ?? null;
      const viewer: OverlayViewer = {
        authorization: self?.visual?.authorization ?? 0,
        telescope: camera.telescope,
        guildWar: ctx.stores.worldStore.getState().worldId === WI_WORLD_GUILDWAR,
        ownGuildId: guild ? guild.idGuild : null,
        // the own and party HP bars (Plan/41 19.4)
        viewHp: true,
        // the siege facts: the defender colour and the best / winner marks (WP-REND-26); null without the events feature
        siege: siegeMarksOf(ctx),
      };
      // the brackets and the charge gauge (WP-REND-20): the targets and gauges the loop set on the renderer, the bound rects of
      // the frame just drawn, and whether the own player flies (the flight sprites of the brackets)
      const view = ctx.sim.view;
      const flying =
        view?.valid === true &&
        self !== undefined &&
        (view.flagsOf(self.objId) & RENDER_VIEW.moverFlags.FLY) !== 0;
      last = renderer.draw({
        subjects,
        projector: chunk.projectorOfCamera(r.cameraRig, w, h),
        viewer,
        nowMs,
        targets: r.targetState,
        gauges: r.gaugeState,
        boundsOf: (id) => r.projectBounds(id),
        flying,
        // the off-screen focus arrow of a flying player points at the focus: its position from the render view
        positionOf: (id) => {
          if (view?.valid !== true || !view.posOf(id, scratchPos)) return null;
          return [scratchPos[0], scratchPos[1], scratchPos[2]];
        },
      });
      frameMs = performance.now() - t0;
      avgMs = avgMs === 0 ? frameMs : avgMs + (frameMs - avgMs) * 0.05;
    },
    tick(due) {
      if (due > 0) renderer?.tick(due);
    },
    countdown(n) {
      renderer?.countdown(n);
    },
    countdownDrawn() {
      return last?.countdowns === 1;
    },
    worldChanged(show) {
      visible = show;
      renderer?.clear();
      last = null;
      if (layer) layer.canvas.style.display = show ? 'block' : 'none';
    },
    dispose() {
      for (const off of offs.splice(0)) off();
      layer?.dispose();
      layer = null;
      renderer = null;
    },
  };
  return host;
}
