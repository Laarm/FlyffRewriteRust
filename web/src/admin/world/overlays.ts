// The vector overlays of the map (Plan/25 section 3.6): regions and titled areas, teleport and revival points, portals, spawn regions,
// continents, the landscape grid, the player heatmap, trails, the atlas artwork, the ruler and the selection box. They are drawn on a 2D canvas
// over the GL canvas from the world meta (`GET /admin/api/world/{id}/meta`); the client holds no coordinates of its own. Everything is
// culled to the viewport, so the cost follows what is in view and not the size of the document.
import type { Coverage } from './tiles';
import type { HeatGrid } from './heatmap';
import type { MetaDoc, RegionDoc } from './metaTypes';
import type { Trail } from './trails';
import { rectsOverlap, worldToScreen, viewRect, type Rect, type View } from './view';

/** `RA_*` flags of `define.rs` that the map colours. */
export const RA = { SAFETY: 16, FIGHT: 64, TOWN: 1024, PK: 4096, DANGER: 16384 } as const;

export interface OverlayToggles {
  readonly regions: boolean;
  readonly titles: boolean;
  readonly teleports: boolean;
  readonly portals: boolean;
  readonly spawns: boolean;
  readonly continents: boolean;
  readonly grid: boolean;
  readonly trails: boolean;
  readonly heatmap: boolean;
  readonly atlas: boolean;
}

export const DEFAULT_TOGGLES: OverlayToggles = {
  regions: true,
  titles: true,
  teleports: true,
  portals: true,
  spawns: false,
  continents: false,
  grid: false,
  trails: true,
  heatmap: false,
  atlas: false,
};

/** Colours read from the console's tokens (they follow the theme). */
export interface OverlayColors {
  /** The colour of a land without a tile (`--map-void`): also the clear colour of the GL canvas. */
  readonly void: string;
  readonly text: string;
  readonly muted: string;
  readonly grid: string;
  readonly accent: string;
  readonly ok: string;
  readonly warn: string;
  readonly danger: string;
  readonly surface: string;
}

export const FALLBACK_COLORS: OverlayColors = {
  void: '#cfd5df',
  text: '#19202e',
  muted: '#566174',
  grid: '#b7bfcc',
  accent: '#2458c5',
  ok: '#17713c',
  warn: '#8a5300',
  danger: '#b3261e',
  surface: '#ffffff',
};

export function readColors(el: Element = document.documentElement): OverlayColors {
  const cs = getComputedStyle(el);
  const get = (name: string, fallback: string): string => {
    const v = cs.getPropertyValue(name).trim();
    return v === '' ? fallback : v;
  };
  return {
    void: get('--map-void', FALLBACK_COLORS.void),
    text: get('--text', FALLBACK_COLORS.text),
    muted: get('--text-muted', FALLBACK_COLORS.muted),
    grid: get('--map-grid', FALLBACK_COLORS.grid),
    accent: get('--accent', FALLBACK_COLORS.accent),
    ok: get('--ok', FALLBACK_COLORS.ok),
    warn: get('--warn', FALLBACK_COLORS.warn),
    danger: get('--danger', FALLBACK_COLORS.danger),
    surface: get('--surface', FALLBACK_COLORS.surface),
  };
}

/** A shape someone else wants on the map (a spawn region of the inspector, a bookmark, the "You" marker). */
export interface Highlight {
  readonly id: string;
  readonly kind: 'rect' | 'point';
  /** `rect`: `[x0, z0, x1, z1]`; `point`: `[x, z, x, z]`. */
  readonly rect: Rect;
  readonly label?: string;
  readonly color?: string;
}

/** An atlas artwork (`Theme/WORLD_*.dds`) and its rectangle `[x1, z1, x2, z2]` of the `worlds` bundle. */
export interface AtlasImage {
  readonly image: CanvasImageSource;
  readonly rect: Rect;
}

export interface OverlayData {
  readonly meta: MetaDoc;
  readonly cov: Coverage;
  readonly toggles: OverlayToggles;
  readonly colors: OverlayColors;
  readonly heat: HeatGrid | null;
  readonly trails: readonly Trail[];
  readonly atlas: readonly AtlasImage[];
  readonly highlights: readonly Highlight[];
  /** The ruler: two points or one (the second follows the cursor). */
  readonly ruler: {
    readonly a: readonly [number, number];
    readonly b: readonly [number, number] | null;
  } | null;
  /** The box being dragged, in world coordinates. */
  readonly box: Rect | null;
  /** The client clock (trails fade by age). */
  readonly now: number;
}

const DASH = [5, 4];

/** Draws every enabled overlay for `view` on `ctx` (the canvas is cleared first; its coordinates are CSS pixels). */
export function drawOverlays(ctx: CanvasRenderingContext2D, view: View, d: OverlayData): void {
  ctx.clearRect(0, 0, view.w, view.h);
  const rect = viewRect(view, 32);
  const t = d.toggles;
  if (t.atlas) drawAtlas(ctx, view, d.atlas);
  if (t.heatmap && d.heat) drawHeat(ctx, view, d.heat, rect);
  drawVoidGrid(ctx, view, d, rect);
  if (t.continents) drawContinents(ctx, view, d, rect);
  if (t.grid) drawGrid(ctx, view, d);
  if (t.regions) drawRegions(ctx, view, d, rect);
  if (t.spawns) drawSpawns(ctx, view, d, rect);
  if (t.portals) drawPortals(ctx, view, d, rect);
  if (t.teleports) drawPoints(ctx, view, d, rect);
  if (t.trails) drawTrails(ctx, view, d);
  drawHighlights(ctx, view, d);
  if (d.ruler) drawRuler(ctx, view, d.ruler, d.colors);
  if (d.box) drawBox(ctx, view, d.box, d.colors);
}

function rectPx(view: View, r: Rect): { x: number; y: number; w: number; h: number } {
  const a = worldToScreen(view, r[0], r[3]);
  const b = worldToScreen(view, r[2], r[1]);
  return { x: a.sx, y: a.sy, w: b.sx - a.sx, h: b.sy - a.sy };
}

function label(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  color: string,
  surface: string,
  align: CanvasTextAlign = 'center',
): void {
  ctx.font = '600 11px system-ui, sans-serif';
  ctx.textAlign = align;
  ctx.textBaseline = 'middle';
  ctx.lineWidth = 3;
  ctx.strokeStyle = surface;
  ctx.globalAlpha = 0.85;
  ctx.strokeText(text, x, y);
  ctx.globalAlpha = 1;
  ctx.fillStyle = color;
  ctx.fillText(text, x, y);
}

// ---- the base: lands without a tile ------------------------------------------------------------------------------------------------

/** The faint landscape grid over the lands that have no minimap tile (Plan/25 section 3.4: "drawn in the void colour with a faint grid"). */
function drawVoidGrid(ctx: CanvasRenderingContext2D, view: View, d: OverlayData, rect: Rect): void {
  const L = d.meta.geometry.land_m;
  if (L / view.upp < 12) return; // lands smaller than 12 px: no grid
  const [nx, nz] = d.meta.geometry.lands;
  const lx0 = Math.max(0, Math.floor(rect[0] / L));
  const lx1 = Math.min(nx - 1, Math.floor(rect[2] / L));
  const lz0 = Math.max(0, Math.floor(rect[1] / L));
  const lz1 = Math.min(nz - 1, Math.floor(rect[3] / L));
  ctx.strokeStyle = d.colors.grid;
  ctx.globalAlpha = 0.35;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let lz = lz0; lz <= lz1; lz += 1) {
    for (let lx = lx0; lx <= lx1; lx += 1) {
      if (d.cov.covered(lx, lz)) continue;
      const p = rectPx(view, [lx * L, lz * L, (lx + 1) * L, (lz + 1) * L]);
      ctx.rect(Math.round(p.x) + 0.5, Math.round(p.y) + 0.5, Math.round(p.w), Math.round(p.h));
    }
  }
  ctx.stroke();
  ctx.globalAlpha = 1;
}

// ---- regions ------------------------------------------------------------------------------------------------------------------------

/** What an attribute area is, for its colour: PK zones red, towns green, safe zones teal, arenas orange, dangerous areas amber. */
export function regionTone(
  r: Pick<RegionDoc, 'attribute' | 'title'>,
  colors: OverlayColors,
): string | null {
  const a = r.attribute;
  if ((a & RA.PK) !== 0) return colors.danger;
  if ((a & RA.TOWN) !== 0) return colors.ok;
  if ((a & RA.SAFETY) !== 0) return colors.ok;
  if ((a & RA.FIGHT) !== 0) return colors.warn;
  if ((a & RA.DANGER) !== 0) return colors.warn;
  return r.title ? colors.accent : null;
}

function drawRegions(ctx: CanvasRenderingContext2D, view: View, d: OverlayData, rect: Rect): void {
  const titles = d.toggles.titles;
  const areas: { r: RegionDoc; area: number }[] = [];
  for (const r of d.meta.regions) {
    // triggers, the begin point and revival areas are drawn by their own overlays
    if (r.index === 10 || r.index === 12 || r.index === 13) continue;
    if (!rectsOverlap(r.rect, rect)) continue;
    const tone = regionTone(r, d.colors);
    if (tone === null) continue;
    areas.push({ r, area: (r.rect[2] - r.rect[0]) * (r.rect[3] - r.rect[1]) });
  }
  // large areas first so that the small ones stay visible on top
  areas.sort((a, b) => b.area - a.area);
  ctx.lineWidth = 1.5;
  for (const { r } of areas) {
    const tone = regionTone(r, d.colors) ?? d.colors.accent;
    const p = rectPx(view, r.rect);
    ctx.globalAlpha = 0.07;
    ctx.fillStyle = tone;
    ctx.fillRect(p.x, p.y, p.w, p.h);
    ctx.globalAlpha = 0.7;
    ctx.strokeStyle = tone;
    ctx.strokeRect(p.x, p.y, p.w, p.h);
    ctx.globalAlpha = 1;
  }
  if (!titles) return;
  for (const { r } of areas) {
    if (!r.title) continue;
    const p = rectPx(view, r.rect);
    // a title needs room: skip it when the area is narrower than the text or off screen
    if (p.w < r.title.length * 5 || p.h < 14) continue;
    if (p.x + p.w < 0 || p.y + p.h < 0 || p.x > view.w || p.y > view.h) continue;
    const cx = Math.min(
      Math.max(p.x + p.w / 2, 8 + (r.title.length * 6) / 2),
      view.w - 8 - (r.title.length * 6) / 2,
    );
    const cy = Math.min(Math.max(p.y + p.h / 2, 12), view.h - 12);
    label(ctx, r.title, cx, cy, d.colors.text, d.colors.surface);
  }
}

// ---- spawn regions --------------------------------------------------------------------------------------------------------------------

function drawSpawns(ctx: CanvasRenderingContext2D, view: View, d: OverlayData, rect: Rect): void {
  ctx.setLineDash(DASH);
  ctx.lineWidth = 1;
  ctx.strokeStyle = d.colors.warn;
  const labelled: { text: string; x: number; y: number }[] = [];
  for (const s of d.meta.spawns) {
    if (!rectsOverlap(s.rect, rect)) continue;
    const p = rectPx(view, s.rect);
    ctx.globalAlpha = 0.8;
    ctx.strokeRect(p.x, p.y, p.w, p.h);
    if (p.w > 60 && p.h > 14) {
      labelled.push({
        text: `${s.name} ×${String(s.count)}${s.level === null ? '' : ` Lv ${String(s.level)}`}`,
        x: p.x + p.w / 2,
        y: p.y + p.h / 2,
      });
    }
  }
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;
  for (const l of labelled.slice(0, 80))
    label(ctx, l.text, l.x, l.y, d.colors.warn, d.colors.surface);
}

// ---- portals, teleport and revival points ---------------------------------------------------------------------------------------------

function drawPortals(ctx: CanvasRenderingContext2D, view: View, d: OverlayData, rect: Rect): void {
  ctx.lineWidth = 1.5;
  for (const p of d.meta.portals) {
    if (!rectsOverlap(p.rect, rect)) continue;
    const r = rectPx(view, p.rect);
    ctx.strokeStyle = d.colors.accent;
    ctx.globalAlpha = 0.9;
    ctx.strokeRect(r.x, r.y, r.w, r.h);
    ctx.fillStyle = d.colors.accent;
    ctx.globalAlpha = 0.12;
    ctx.fillRect(r.x, r.y, r.w, r.h);
    ctx.globalAlpha = 1;
    // an arrow in the middle of the area, pointing up-right ("towards the other world")
    const cx = r.x + r.w / 2;
    const cy = r.y + r.h / 2;
    ctx.beginPath();
    ctx.moveTo(cx - 7, cy + 7);
    ctx.lineTo(cx + 7, cy - 7);
    ctx.moveTo(cx + 1, cy - 7);
    ctx.lineTo(cx + 7, cy - 7);
    ctx.lineTo(cx + 7, cy - 1);
    ctx.stroke();
    label(ctx, `→ ${p.to_name}`, cx, cy + 16, d.colors.accent, d.colors.surface);
  }
}

function pin(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  color: string,
  shape: 'diamond' | 'cross',
): void {
  ctx.beginPath();
  if (shape === 'diamond') {
    ctx.moveTo(x, y - 7);
    ctx.lineTo(x + 6, y);
    ctx.lineTo(x, y + 7);
    ctx.lineTo(x - 6, y);
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.fill();
    ctx.strokeStyle = '#fff';
    ctx.lineWidth = 1.5;
    ctx.stroke();
  } else {
    ctx.moveTo(x - 6, y);
    ctx.lineTo(x + 6, y);
    ctx.moveTo(x, y - 6);
    ctx.lineTo(x, y + 6);
    ctx.lineWidth = 3.5;
    ctx.strokeStyle = '#fff';
    ctx.stroke();
    ctx.lineWidth = 2;
    ctx.strokeStyle = color;
    ctx.stroke();
  }
}

function drawPoints(ctx: CanvasRenderingContext2D, view: View, d: OverlayData, rect: Rect): void {
  const inside = (x: number, z: number): boolean =>
    x >= rect[0] && x <= rect[2] && z >= rect[1] && z <= rect[3];
  for (const t of d.meta.teleports) {
    if (!inside(t.pos[0], t.pos[2])) continue;
    const { sx, sy } = worldToScreen(view, t.pos[0], t.pos[2]);
    pin(ctx, sx, sy, d.colors.accent, 'diamond');
    label(ctx, t.name, sx, sy + 15, d.colors.text, d.colors.surface);
  }
  for (const r of d.meta.revival) {
    if (!inside(r.pos[0], r.pos[2])) continue;
    const { sx, sy } = worldToScreen(view, r.pos[0], r.pos[2]);
    pin(ctx, sx, sy, d.colors.ok, 'cross');
    label(ctx, r.title ?? r.key, sx, sy + 14, d.colors.ok, d.colors.surface);
  }
}

// ---- continents and the grid -----------------------------------------------------------------------------------------------------------

function drawContinents(
  ctx: CanvasRenderingContext2D,
  view: View,
  d: OverlayData,
  rect: Rect,
): void {
  ctx.lineWidth = 1.5;
  for (const c of d.meta.continents) {
    if (c.polygon.length < 3) continue;
    let x0 = Infinity;
    let z0 = Infinity;
    let x1 = -Infinity;
    let z1 = -Infinity;
    for (const [x, z] of c.polygon) {
      x0 = Math.min(x0, x);
      z0 = Math.min(z0, z);
      x1 = Math.max(x1, x);
      z1 = Math.max(z1, z);
    }
    if (!rectsOverlap([x0, z0, x1, z1], rect)) continue;
    ctx.beginPath();
    c.polygon.forEach(([x, z], i) => {
      const { sx, sy } = worldToScreen(view, x, z);
      if (i === 0) ctx.moveTo(sx, sy);
      else ctx.lineTo(sx, sy);
    });
    ctx.closePath();
    const tone = c.town ? d.colors.ok : d.colors.accent;
    ctx.globalAlpha = 0.08;
    ctx.fillStyle = tone;
    ctx.fill();
    ctx.globalAlpha = 0.7;
    ctx.strokeStyle = tone;
    ctx.stroke();
    ctx.globalAlpha = 1;
    const mid = worldToScreen(view, (x0 + x1) / 2, (z0 + z1) / 2);
    label(ctx, c.name, mid.sx, mid.sy, d.colors.text, d.colors.surface);
  }
}

/** The spacing of the grid labels: the smallest of 100 m, 500 m, 1 km, 5 km, 10 km whose labels are at least 80 px apart. */
export function gridLabelStep(upp: number): number {
  for (const m of [100, 500, 1000, 5000, 10000, 50000]) if (m / upp >= 80) return m;
  return 50000;
}

function drawGrid(ctx: CanvasRenderingContext2D, view: View, d: OverlayData): void {
  const L = d.meta.geometry.land_m;
  const [ex, ez] = d.meta.geometry.extent_m;
  const r = viewRect(view, 0);
  if (L / view.upp >= 6) {
    ctx.strokeStyle = d.colors.grid;
    ctx.globalAlpha = 0.8;
    ctx.lineWidth = 1;
    ctx.beginPath();
    const x0 = Math.max(0, Math.ceil(r[0] / L) * L);
    const x1 = Math.min(ex, r[2]);
    const z0 = Math.max(0, Math.ceil(r[1] / L) * L);
    const z1 = Math.min(ez, r[3]);
    for (let x = x0; x <= x1; x += L) {
      const { sx } = worldToScreen(view, x, 0);
      ctx.moveTo(Math.round(sx) + 0.5, 0);
      ctx.lineTo(Math.round(sx) + 0.5, view.h);
    }
    for (let z = z0; z <= z1; z += L) {
      const { sy } = worldToScreen(view, 0, z);
      ctx.moveTo(0, Math.round(sy) + 0.5);
      ctx.lineTo(view.w, Math.round(sy) + 0.5);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
  const step = gridLabelStep(view.upp);
  const kx = Math.ceil(Math.max(0, r[0]) / step) * step;
  for (let x = kx; x <= Math.min(ex, r[2]); x += step) {
    const { sx } = worldToScreen(view, x, 0);
    label(
      ctx,
      x >= 1000 ? `${String(x / 1000)} km` : `${String(x)} m`,
      sx,
      10,
      d.colors.muted,
      d.colors.surface,
    );
  }
  const kz = Math.ceil(Math.max(0, r[1]) / step) * step;
  for (let z = kz; z <= Math.min(ez, r[3]); z += step) {
    const { sy } = worldToScreen(view, 0, z);
    label(
      ctx,
      z >= 1000 ? `${String(z / 1000)} km` : `${String(z)} m`,
      6,
      sy,
      d.colors.muted,
      d.colors.surface,
      'left',
    );
  }
}

// ---- heatmap, trails, atlas ---------------------------------------------------------------------------------------------------------------

function drawHeat(ctx: CanvasRenderingContext2D, view: View, heat: HeatGrid, rect: Rect): void {
  const max = heat.max;
  if (max <= 0) return;
  const cell = heat.cellM / view.upp;
  for (const c of heat.cellsIn(rect)) {
    const f = Math.log1p(c.value) / Math.log1p(max);
    const { sx, sy } = worldToScreen(view, c.x0, c.z0 + heat.cellM);
    ctx.globalAlpha = 0.15 + 0.6 * f;
    ctx.fillStyle = heatColor(f);
    ctx.fillRect(sx, sy, Math.ceil(cell) + 0.5, Math.ceil(cell) + 0.5);
  }
  ctx.globalAlpha = 1;
}

/** The colour of a heat value `f` in 0..1: blue-ish through green to red. */
export function heatColor(f: number): string {
  const c = Math.min(1, Math.max(0, f));
  const h = Math.round(240 - 240 * c);
  return `hsl(${String(h)} 90% 50%)`;
}

function drawTrails(ctx: CanvasRenderingContext2D, view: View, d: OverlayData): void {
  ctx.lineWidth = 2;
  ctx.lineCap = 'round';
  for (const tr of d.trails) {
    const pts = tr.points(d.now);
    if (pts.length < 2) continue;
    for (let i = 1; i < pts.length; i += 1) {
      const a = pts[i - 1];
      const b = pts[i];
      if (!a || !b) continue;
      const pa = worldToScreen(view, a.x, a.z);
      const pb = worldToScreen(view, b.x, b.z);
      ctx.globalAlpha = 0.15 + 0.7 * b.age01;
      ctx.strokeStyle = d.colors.accent;
      ctx.beginPath();
      ctx.moveTo(pa.sx, pa.sy);
      ctx.lineTo(pb.sx, pb.sy);
      ctx.stroke();
    }
  }
  ctx.globalAlpha = 1;
}

function drawAtlas(ctx: CanvasRenderingContext2D, view: View, atlas: readonly AtlasImage[]): void {
  ctx.globalAlpha = 0.55;
  for (const a of atlas) {
    const p = rectPx(view, [a.rect[0], a.rect[1], a.rect[2], a.rect[3]]);
    try {
      ctx.drawImage(a.image, p.x, p.y, p.w, p.h);
    } catch {
      // an image that cannot be drawn (a closed bitmap) is skipped
    }
  }
  ctx.globalAlpha = 1;
}

// ---- highlights, ruler, box ---------------------------------------------------------------------------------------------------------------

function drawHighlights(ctx: CanvasRenderingContext2D, view: View, d: OverlayData): void {
  for (const h of d.highlights) {
    const color = h.color ?? d.colors.warn;
    if (h.kind === 'rect') {
      const p = rectPx(view, h.rect);
      ctx.lineWidth = 2;
      ctx.strokeStyle = color;
      ctx.setLineDash(DASH);
      ctx.strokeRect(p.x, p.y, p.w, p.h);
      ctx.setLineDash([]);
      ctx.globalAlpha = 0.1;
      ctx.fillStyle = color;
      ctx.fillRect(p.x, p.y, p.w, p.h);
      ctx.globalAlpha = 1;
      if (h.label) label(ctx, h.label, p.x + p.w / 2, p.y - 8, color, d.colors.surface);
    } else {
      const { sx, sy } = worldToScreen(view, h.rect[0], h.rect[1]);
      ctx.beginPath();
      ctx.arc(sx, sy, 9, 0, Math.PI * 2);
      ctx.lineWidth = 2.5;
      ctx.strokeStyle = color;
      ctx.stroke();
      if (h.label) label(ctx, h.label, sx, sy - 16, color, d.colors.surface);
    }
  }
}

/** The distance in metres and the walking time at 5 m/s of the ruler (25 section 3.9). */
export function rulerReadout(
  a: readonly [number, number],
  b: readonly [number, number],
): { metres: number; seconds: number } {
  const metres = Math.hypot(b[0] - a[0], b[1] - a[1]);
  return { metres, seconds: metres / 5 };
}

export function formatRuler(r: { metres: number; seconds: number }): string {
  const m =
    r.metres >= 1000 ? `${(r.metres / 1000).toFixed(2)} km` : `${String(Math.round(r.metres))} m`;
  const s = Math.round(r.seconds);
  const t = s >= 60 ? `${String(Math.floor(s / 60))} min ${String(s % 60)} s` : `${String(s)} s`;
  return `${m} · ${t} walking`;
}

function drawRuler(
  ctx: CanvasRenderingContext2D,
  view: View,
  ruler: { a: readonly [number, number]; b: readonly [number, number] | null },
  colors: OverlayColors,
): void {
  const a = worldToScreen(view, ruler.a[0], ruler.a[1]);
  ctx.fillStyle = colors.danger;
  ctx.beginPath();
  ctx.arc(a.sx, a.sy, 4, 0, Math.PI * 2);
  ctx.fill();
  if (!ruler.b) return;
  const b = worldToScreen(view, ruler.b[0], ruler.b[1]);
  ctx.strokeStyle = colors.danger;
  ctx.lineWidth = 2;
  ctx.setLineDash(DASH);
  ctx.beginPath();
  ctx.moveTo(a.sx, a.sy);
  ctx.lineTo(b.sx, b.sy);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.beginPath();
  ctx.arc(b.sx, b.sy, 4, 0, Math.PI * 2);
  ctx.fill();
  label(
    ctx,
    formatRuler(rulerReadout(ruler.a, ruler.b)),
    (a.sx + b.sx) / 2,
    (a.sy + b.sy) / 2 - 12,
    colors.danger,
    colors.surface,
  );
}

function drawBox(
  ctx: CanvasRenderingContext2D,
  view: View,
  box: Rect,
  colors: OverlayColors,
): void {
  const p = rectPx(view, box);
  ctx.globalAlpha = 0.12;
  ctx.fillStyle = colors.accent;
  ctx.fillRect(p.x, p.y, p.w, p.h);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = colors.accent;
  ctx.lineWidth = 1.5;
  ctx.setLineDash(DASH);
  ctx.strokeRect(p.x, p.y, p.w, p.h);
  ctx.setLineDash([]);
}
