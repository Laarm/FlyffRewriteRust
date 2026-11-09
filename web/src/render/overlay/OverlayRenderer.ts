// `OverlayRenderer` (Plan/41 §19.1; pass 17): the in-world 2-D layer. Every frame it clears its canvas and draws, for every
// mover the world host reports, the HP bar, the name lines, the stun icon, the speech balloon / emoticon / vendor title,
// and then the floating damage numbers. The visual decisions are the pure functions of `Nameplates`, `HpBars`,
// `Balloons` and `DamageNumbers`; this class owns the registries that live across frames (balloons, damage numbers),
// resolves the textures through `OverlayAssets` and paints the items with Canvas2D.
//
// DEVIATION (Plan/41 §19.1 draws the layer as one sprite batch of the WebGL pipeline with a glyph atlas): the layer is a
// Canvas2D canvas stacked above the WebGL canvas. Glyphs are rasterised by the browser; widths, positions, colours,
// sizes, outlines and alphas follow the original (see GlyphAtlas.ts). The gamma ramp of pass 16 does not reach this
// canvas: `colorMap` (the ramp LUT) is applied to the colours the renderer chooses (names, balloon alpha stays).

import type { RenderFx } from '../fx';
import type { GaugeState, ObjId, ScreenRect, TargetState, Vec3LH } from '../types';
import type { Projector } from './anchor';
import { BalloonLayer, type BalloonKind } from './Balloons';
import { DamageNumbers } from './DamageNumbers';
import {
  type Ctx2D,
  drawOutlinedText,
  GlyphMeter,
  OVERLAY_FONTS,
  type TextMeter,
} from './GlyphAtlas';
import { layoutTargetBrackets } from './Brackets';
import { layoutQuestMark, QuestPulse, wantsQuestMark } from './QuestMarks';
import { COUNTDOWN_ATLAS, COUNTDOWN_MS, layoutCountdown } from './Countdown';
import { layoutFlyTargetRects, layoutFocusArrow } from './FlightHud';
import { ACCEL_INNER, layoutGauges } from './Gauges';
import { hpBarShown, layoutHpBar } from './HpBars';
import {
  DEFAULT_NAME_COLORS,
  DEFAULT_NAME_OPTIONS,
  DEFAULT_NAME_TITLES,
  layoutNameplate,
  type NameColors,
  type NameOptions,
  type NameTitles,
  stunIcon,
} from './Nameplates';
import type { DrawItem, OverlayAssets, OverlaySubject, OverlayViewer } from './types';

/**
 * The exact integer matrices `(a, b, c, d)` of the clockwise quarter turns on a y-down canvas (BFIX1 #001): a picture that points
 * down-right `(1, 1)` points down-left `(-1, 1)` after 90, up-left `(-1, -1)` after 180 and up-right `(1, -1)` after 270. No cosines,
 * so a square box lands on itself pixel for pixel.
 */
export const QUARTER_TURNS: Readonly<
  Record<90 | 180 | 270, readonly [number, number, number, number]>
> = {
  90: [0, 1, -1, 0],
  180: [-1, 0, 0, -1],
  270: [0, -1, 1, 0],
};

/** The part of `RenderFx` the overlay implements. */
export interface OverlayFx {
  balloon(id: ObjId, b: { text: string; kind: BalloonKind } | null): void;
  emoticon(id: ObjId, set: 'chat' | 'dialog', index: number): void;
  vendorTitle(id: ObjId, title: string | null): void;
  damageNumber(posLH: Vec3LH, value: number, style: 0 | 1): void;
}

/** What one frame of the overlay is told. */
export interface OverlayFrame {
  readonly subjects: Iterable<OverlaySubject>;
  readonly projector: Projector;
  readonly viewer: OverlayViewer;
  readonly nowMs: number;
  /** The target brackets of the frame (`renderer.setTargets`, WP-REND-20). */
  readonly targets?: TargetState | null;
  /** The own player's world gauges (`renderer.setGauges`). */
  readonly gauges?: GaugeState | null;
  /** The projected bound rect of an object in CSS pixels, `null` when it is not on the screen (`renderer.projectBounds`). */
  readonly boundsOf?: (id: ObjId) => ScreenRect | null;
  /** The own player flies: the brackets use the flight sprites and offsets. */
  readonly flying?: boolean;
  /**
   * The LH position of a mover (the focus arrow of a flying player points at an off-screen focus); `null` when it is not known. Absent:
   * no arrow.
   */
  readonly positionOf?: (id: ObjId) => Vec3LH | null;
}

/** What the last frame drew (the probe and the tests). */
export interface OverlayStats {
  subjects: number;
  nameplates: number;
  hpBars: number;
  balloons: number;
  vendorTitles: number;
  damageNumbers: number;
  /** Bracket corner sprites drawn (4 per bracketed object). */
  brackets: number;
  /** Items of the own player's world gauges. */
  gauges: number;
  /** Guild mark pictures drawn above players: master / hero icons, logos, best / winner marks (WP-REND-26). */
  marks: number;
  /** Quest emoticons drawn over NPCs (WP-REND-27). */
  questMarks: number;
  /** Red outlines around the fly targets, the off-screen focus arrow (0 or 1), the PvP countdown sprite (0 or 1). */
  flyRects: number;
  focusArrows: number;
  countdowns: number;
  textItems: number;
  spriteItems: number;
  /** Rectangle and polygon items painted. */
  rectItems: number;
  missingAssets: number;
}

export interface OverlayRendererOptions {
  readonly ctx: Ctx2D;
  readonly assets: OverlayAssets;
  readonly meter?: TextMeter;
  /** The clock of `balloon()` / `emoticon()` requests (`performance.now()`). */
  readonly now?: () => number;
  /** The colour of vendor titles (red or blue). */
  readonly rng?: () => number;
  /** The gamma ramp applied to colours (`(argb) => argb`). */
  readonly colorMap?: (argb: number) => number;
}

/** One text the last frame drew (the probe `overlay.texts()`: where the names and balloon lines are on the screen). */
export interface DrawnText {
  readonly text: string;
  readonly x: number;
  readonly y: number;
  /** `0xAARRGGBB` before the gamma ramp (the guild name colours of WP-REND-26 are asserted through it). */
  readonly color: number;
}

/** One guild mark picture the last frame drew (the probe `overlay.marks()`): the mover it belongs to, the picture and its screen position. */
export interface DrawnMark {
  readonly id: number;
  readonly path: string;
  readonly x: number;
  readonly y: number;
}

/** One quest emoticon the last frame drew (the probe `overlay.questMarks()`): the NPC, the picture index, the screen position and the pulse scale. */
export interface DrawnQuestMark {
  readonly id: number;
  readonly index: number;
  readonly x: number;
  readonly y: number;
  readonly scale: number;
}

export interface OverlayConfig {
  names: NameOptions;
  colors: NameColors;
  titles: NameTitles;
  /** `features.damage_space`. */
  damageSpace: boolean;
}

export const DEFAULT_OVERLAY_CONFIG: OverlayConfig = {
  names: DEFAULT_NAME_OPTIONS,
  colors: DEFAULT_NAME_COLORS,
  titles: DEFAULT_NAME_TITLES,
  damageSpace: false,
};

export class OverlayRenderer implements OverlayFx {
  readonly balloons: BalloonLayer;
  readonly damage = new DamageNumbers();
  readonly meter: TextMeter;
  config: OverlayConfig = DEFAULT_OVERLAY_CONFIG;
  #ctx: Ctx2D;
  readonly #assets: OverlayAssets;
  readonly #now: () => number;
  readonly #map: (argb: number) => number;
  #projector: Projector | null = null;
  #lastStats: OverlayStats = emptyStats();
  /** The texts of the frame being drawn / the last frame, while `recordTexts` is on (probe and tests). */
  #texts: DrawnText[] = [];
  #lastTexts: readonly DrawnText[] = [];
  /** Records the texts of every frame for `drawnTexts` (off by default: no allocation per text in the game). */
  recordTexts = false;
  /** The guild mark pictures of the frame being drawn / the last frame, while `recordMarks` is on. */
  #marks: DrawnMark[] = [];
  #lastMarks: readonly DrawnMark[] = [];
  /** Records the guild marks of every frame for `drawnMarks` (off by default). */
  recordMarks = false;
  /** The ONE global pulse of the quest emoticons (`ProcessQuestEmoticon`, per tick). */
  readonly questPulse = new QuestPulse();
  /** The quest emoticons of the frame being drawn / the last frame, while `recordQuestMarks` is on. */
  #questMarks: DrawnQuestMark[] = [];
  #lastQuestMarks: readonly DrawnQuestMark[] = [];
  recordQuestMarks = false;
  /** 60 Hz cosmetic ticks since the start: the parity makes the turbo bar blink (WP-REND-25). */
  tickCount = 0;
  /** The PvP countdown on the screen: the number and the `nowMs` of its message (`null`: none). */
  #countdown: { n: number; at: number } | null = null;
  #width = 0;
  #height = 0;

  constructor(o: OverlayRendererOptions) {
    this.#ctx = o.ctx;
    this.#assets = o.assets;
    this.meter = o.meter ?? new GlyphMeter();
    this.#now = o.now ?? (() => performance.now());
    this.#map = o.colorMap ?? ((c) => c);
    this.balloons = new BalloonLayer(this.meter, o.rng);
  }

  /** The device context changed (a lost 2-D context was restored). */
  setContext(ctx: Ctx2D): void {
    this.#ctx = ctx;
  }

  configure(c: Partial<OverlayConfig>): void {
    this.config = { ...this.config, ...c };
  }

  get stats(): OverlayStats {
    return this.#lastStats;
  }

  /** The texts the last frame drew (empty unless `recordTexts` is on). */
  get drawnTexts(): readonly DrawnText[] {
    return this.#lastTexts;
  }

  /** The guild marks the last frame drew (empty unless `recordMarks` is on). */
  get drawnMarks(): readonly DrawnMark[] {
    return this.#lastMarks;
  }

  /** The quest emoticons the last frame drew (empty unless `recordQuestMarks` is on). */
  get drawnQuestMarks(): readonly DrawnQuestMark[] {
    return this.#lastQuestMarks;
  }

  // ---- the RenderFx surface ------------------------------------------------------------------------------------

  balloon(id: ObjId, b: { text: string; kind: BalloonKind } | null): void {
    if (b === null) this.balloons.remove(id);
    else this.balloons.add(id, b.text, b.kind, this.#now());
  }

  emoticon(id: ObjId, set: 'chat' | 'dialog', index: number): void {
    this.balloons.addEmoticon(id, set, index, this.#now());
  }

  vendorTitle(id: ObjId, title: string | null): void {
    if (title === null || title === '') this.balloons.removeVendor(id);
    else this.balloons.addVendor(id, title);
  }

  damageNumber(posLH: Vec3LH, value: number, style: 0 | 1): void {
    this.damage.add(posLH, value, style);
  }

  /** The object was deleted: its balloon, emoticon and vendor title go. */
  removeFor(id: ObjId): void {
    this.balloons.removeFor(id);
  }

  /** The world changed: everything floating goes (`ClearAllMessage`). */
  clear(): void {
    this.balloons.clear();
    this.damage.clear();
    this.#countdown = null;
  }

  /** `steps` cosmetic ticks (60 Hz). */
  tick(steps: number): void {
    for (let i = 0; i < steps; i += 1) {
      this.damage.tick(this.#projector);
      this.questPulse.tick();
      this.tickCount += 1;
    }
  }

  /** `DuelCount`: shows the countdown number `n` (3, 2, 1; 0 is the word "fight") from now on for 1.5 s. */
  countdown(n: number): void {
    this.#countdown = { n, at: this.#now() };
  }

  // ---- drawing -------------------------------------------------------------------------------------------------

  /** Resizes the canvas coordinate space the clear covers (CSS pixels). */
  resize(width: number, height: number): void {
    this.#width = width;
    this.#height = height;
  }

  draw(frame: OverlayFrame): OverlayStats {
    const stats = emptyStats();
    this.#texts = [];
    this.#marks = [];
    this.#questMarks = [];
    const { projector, viewer, nowMs } = frame;
    this.#projector = projector;
    this.balloons.expire(nowMs);
    const ctx = this.#ctx;
    ctx.clearRect(0, 0, this.#width || projector.width, this.#height || projector.height);
    const c = this.config;
    const nameCtx = {
      viewer,
      options: c.names,
      colors: c.colors,
      titles: c.titles,
      meter: this.meter,
      assets: this.#assets,
    };
    for (const s of frame.subjects) {
      stats.subjects += 1;
      if (s.culled) continue;
      const anchor = projector.project(s.head);
      if (!anchor) continue;
      const items: DrawItem[] = [];
      if (hpBarShown(s, viewer, c.names, true)) {
        const fill = this.#assets.image('theme/gaupartyhp.tga');
        items.push(...layoutHpBar(s, anchor, fill?.w ?? 0));
        stats.hpBars += 1;
      }
      const plate = layoutNameplate(s, anchor, nameCtx);
      if (plate.length > 0) stats.nameplates += 1;
      for (const it of plate) {
        // the guild mark pictures: icon, logo, best / winner mark (the plate has no other images)
        if (it.kind !== 'image') continue;
        stats.marks += 1;
        if (this.recordMarks) this.#marks.push({ id: s.id, path: it.path, x: it.x, y: it.y });
      }
      items.push(...plate);
      const stun = stunIcon(s, anchor);
      if (stun) items.push(stun);
      if (s.isSelf && frame.gauges) {
        const inner = this.#assets.image(ACCEL_INNER)?.w ?? 0;
        const foot = s.foot ? projector.project(s.foot) : null;
        const gauge = layoutGauges(anchor, frame.gauges, inner, this.tickCount, foot ?? undefined);
        stats.gauges += gauge.length;
        items.push(...gauge);
      }
      const floating = this.balloons.layoutFor(s.id, anchor, nowMs);
      if (floating) {
        items.push(...floating);
        if (this.balloons.hasVendor(s.id)) stats.vendorTitles += 1;
        else stats.balloons += 1;
      }
      // the quest emoticon of an NPC (WP-REND-27): hidden while it has a speech balloon
      if (wantsQuestMark(s, this.balloons.has(s.id))) {
        const mark = layoutQuestMark(anchor, s.questEmoticon ?? 0, this.questPulse.scale);
        if (mark) {
          items.push(mark);
          stats.questMarks += 1;
          if (this.recordQuestMarks) {
            this.#questMarks.push({
              id: s.id,
              index: mark.index,
              x: mark.x,
              y: mark.y,
              scale: this.questPulse.scale,
            });
          }
        }
      }
      this.#paint(items, stats);
    }
    const boundsOf = frame.boundsOf;
    if (frame.targets && boundsOf) {
      const brackets = layoutTargetBrackets({
        targets: frame.targets,
        flying: frame.flying === true,
        boundsOf,
      });
      stats.brackets = brackets.length;
      this.#paint(brackets, stats);
    }
    if (frame.flying === true && boundsOf) {
      // the flight HUD: the red rectangles of the fly targets and the arrow to an off-screen focus
      const rects = layoutFlyTargetRects(frame.targets?.flyTargets ?? [], boundsOf);
      stats.flyRects = rects.length;
      this.#paint(rects, stats);
      const focus = frame.targets?.focus ?? 0;
      const where = focus !== 0 ? (frame.positionOf?.(focus) ?? null) : null;
      if (where !== null && boundsOf(focus) === null) {
        const arrow = layoutFocusArrow({
          width: projector.width,
          height: projector.height,
          target: projector.project(where),
        });
        if (arrow) {
          stats.focusArrows = 1;
          this.#paint([arrow], stats);
        }
      }
    }
    const numbers = this.damage.layout(projector, this.#assets, c.damageSpace);
    stats.damageNumbers = this.damage.count;
    this.#paint(numbers, stats);
    this.#paintCountdown(nowMs, projector, stats);
    ctx.globalAlpha = 1;
    this.#lastStats = stats;
    this.#lastTexts = this.#texts;
    this.#lastMarks = this.#marks;
    this.#lastQuestMarks = this.#questMarks;
    return stats;
  }

  /** The PvP countdown sprite: its size is the loaded sprite's, so the layout waits for the texture. */
  #paintCountdown(nowMs: number, projector: Projector, stats: OverlayStats): void {
    const c = this.#countdown;
    if (c === null) return;
    const elapsed = nowMs - c.at;
    if (elapsed > COUNTDOWN_MS) {
      this.#countdown = null;
      return;
    }
    const index = c.n === 0 ? 10 : c.n;
    const sprite = this.#assets.sprite(COUNTDOWN_ATLAS, index);
    if (!sprite) return;
    const item = layoutCountdown(c.n, elapsed, projector, sprite);
    if (!item) return;
    stats.countdowns = 1;
    this.#paint([item], stats);
  }

  #paint(items: readonly DrawItem[], stats: OverlayStats): void {
    const ctx = this.#ctx;
    for (const it of items) {
      if (it.kind === 'rect') {
        ctx.globalAlpha = ((it.color >>> 24) & 0xff) / 255;
        ctx.fillStyle = cssRgb(this.#map(it.color));
        if (it.filled) {
          ctx.fillRect(it.x, it.y, it.w, it.h);
        } else {
          // `RenderRect`: four one pixel lines
          ctx.fillRect(it.x, it.y, it.w + 1, 1);
          ctx.fillRect(it.x, it.y + it.h, it.w + 1, 1);
          ctx.fillRect(it.x, it.y, 1, it.h + 1);
          ctx.fillRect(it.x + it.w, it.y, 1, it.h + 1);
        }
        stats.rectItems += 1;
        continue;
      }
      if (it.kind === 'poly') {
        ctx.globalAlpha = ((it.color >>> 24) & 0xff) / 255;
        ctx.fillStyle = cssRgb(this.#map(it.color));
        fillPolygon(ctx, it.points);
        stats.rectItems += 1;
        continue;
      }
      if (it.kind === 'text') {
        ctx.globalAlpha = 1;
        drawOutlinedText(ctx, it.font, it.text, it.x, it.y, this.#map(it.color));
        stats.textItems += 1;
        if (this.recordTexts) {
          this.#texts.push({ text: it.text, x: it.x, y: it.y, color: it.color >>> 0 });
        }
        continue;
      }
      const img =
        it.kind === 'sprite'
          ? this.#assets.sprite(it.atlas, it.index)
          : this.#assets.image(it.path);
      if (!img) {
        stats.missingAssets += 1;
        continue;
      }
      ctx.imageSmoothingEnabled = it.kind === 'image';
      ctx.globalAlpha = Math.max(0, Math.min(255, it.alpha)) / 255;
      let sw = img.w;
      if (it.kind === 'image' && it.cropW !== undefined)
        sw = Math.max(0, Math.min(img.w, it.cropW));
      if (sw <= 0) continue;
      const scaleX = it.kind === 'sprite' ? (it.scale ?? 1) : it.scaleX;
      const scaleY = it.kind === 'sprite' ? (it.scale ?? 1) : it.scaleY;
      const dw = it.dw ?? sw * scaleX;
      const dh = it.dh ?? img.h * scaleY;
      const turn = it.kind === 'sprite' && it.rotate ? QUARTER_TURNS[it.rotate] : undefined;
      if (turn) {
        // BFIX1 #001: the same picture turned about the centre of its box by an exact integer matrix (`transform` multiplies the
        // device-pixel-ratio scale of the layer, `setTransform` would drop it); a square box lands on itself pixel for pixel
        ctx.save();
        ctx.transform(turn[0], turn[1], turn[2], turn[3], it.x + dw / 2, it.y + dh / 2);
        ctx.drawImage(img.source, img.sx, img.sy, sw, img.h, -dw / 2, -dh / 2, dw, dh);
        ctx.restore();
      } else {
        ctx.drawImage(img.source, img.sx, img.sy, sw, img.h, it.x, it.y, dw, dh);
      }
      stats.spriteItems += 1;
    }
  }
}

function emptyStats(): OverlayStats {
  return {
    subjects: 0,
    nameplates: 0,
    hpBars: 0,
    balloons: 0,
    vendorTitles: 0,
    damageNumbers: 0,
    brackets: 0,
    gauges: 0,
    marks: 0,
    questMarks: 0,
    flyRects: 0,
    focusArrows: 0,
    countdowns: 0,
    rectItems: 0,
    textItems: 0,
    spriteItems: 0,
    missingAssets: 0,
  };
}

/**
 * `RenderFx` whose overlay part (`balloon`, `emoticon`, `vendorTitle`, `damageNumber`) goes to the overlay renderer and
 * everything else to `base` (the sfx, shake and projectile requests of the other passes). The base still sees the
 * overlay requests too, so its counters keep working.
 */
export function composeFx(base: RenderFx, overlay: OverlayFx): RenderFx {
  return {
    sfx: (xi, o) => base.sfx(xi, o),
    removeSfx: (h) => {
      base.removeSfx(h);
    },
    markRemoveSfx: (anchor, xi) => {
      base.markRemoveSfx(anchor, xi);
    },
    damageNumber: (pos, value, style) => {
      overlay.damageNumber(pos, value, style);
      base.damageNumber(pos, value, style);
    },
    hitShake: (id) => {
      base.hitShake(id);
    },
    projectileVisual: (pid, sfx, kind, target, attacker) => {
      base.projectileVisual(pid, sfx, kind, target, attacker);
    },
    projectileEnd: (pid, at, hit) => {
      base.projectileEnd(pid, at, hit);
    },
    balloon: (id, b) => {
      overlay.balloon(id, b);
      base.balloon(id, b);
    },
    emoticon: (id, set, index) => {
      overlay.emoticon(id, set, index);
      base.emoticon(id, set, index);
    },
    vendorTitle: (id, title) => {
      overlay.vendorTitle(id, title);
      base.vendorTitle(id, title);
    },
    moveMark: (pos, normal) => {
      base.moveMark(pos, normal);
    },
    meleeSparks: (attacker, option) => {
      base.meleeSparks?.(attacker, option);
    },
    skillYoyo: (src, target, sfxId, hands) => {
      base.skillYoyo?.(src, target, sfxId, hands);
    },
    hotAir: (o) => {
      base.hotAir?.(o);
    },
    itemDropped: (id) => {
      base.itemDropped?.(id);
    },
    ctrlPlace: (id, kind) => {
      base.ctrlPlace?.(id, kind);
    },
    // ---- render-audio-m7 (WP-REND-27)
    bossMeleeHit: (attacker, target, state, hitCount) => {
      base.bossMeleeHit?.(attacker, target, state, hitCount);
    },
    bossCharge: (id, state) => {
      base.bossCharge?.(id, state);
    },
    eventBalloon: (playerId, itemId) => {
      base.eventBalloon?.(playerId, itemId);
    },
    roomPapering: (o) => {
      base.roomPapering?.(o);
    },
  };
}

export { OVERLAY_FONTS };

/** `#rrggbb` of an `0xAARRGGBB` colour (the alpha goes through `globalAlpha`). */
function cssRgb(argb: number): string {
  return `#${(argb & 0xffffff).toString(16).padStart(6, '0')}`;
}

/** Fills a convex polygon with one pixel rows (`Ctx2D` has no path API): the scanlines between its edges. */
export function fillPolygon(ctx: Pick<Ctx2D, 'fillRect'>, pts: readonly number[]): void {
  const n = Math.floor(pts.length / 2);
  if (n < 3) return;
  let minY = Number.POSITIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (let i = 0; i < n; i += 1) {
    const y = pts[i * 2 + 1] ?? 0;
    if (y < minY) minY = y;
    if (y > maxY) maxY = y;
  }
  for (let y = Math.ceil(minY); y <= Math.floor(maxY); y += 1) {
    let x0 = Number.POSITIVE_INFINITY;
    let x1 = Number.NEGATIVE_INFINITY;
    for (let i = 0; i < n; i += 1) {
      const ax = pts[i * 2] ?? 0;
      const ay = pts[i * 2 + 1] ?? 0;
      const bx = pts[((i + 1) % n) * 2] ?? 0;
      const by = pts[((i + 1) % n) * 2 + 1] ?? 0;
      if ((ay <= y && by >= y) || (by <= y && ay >= y)) {
        const x = ay === by ? Math.min(ax, bx) : ax + ((y - ay) * (bx - ax)) / (by - ay);
        if (x < x0) x0 = x;
        if (x > x1) x1 = x;
        if (ay === by) {
          const xm = Math.max(ax, bx);
          if (xm > x1) x1 = xm;
        }
      }
    }
    if (x1 >= x0) ctx.fillRect(Math.round(x0), y, Math.max(1, Math.round(x1 - x0) + 1), 1);
  }
}
