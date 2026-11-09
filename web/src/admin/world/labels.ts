// The labels of the map (Plan/25 section 3.5): at most 150 DOM labels at once, chosen by priority (selected, followed, the acting character,
// players, then NPCs nearest to the centre when close), hidden above 4 u/px except the selection, the followed one and the acting character,
// placed without overlapping on a coarse 24 px grid. Monsters get a label only when followed or when they are the single selected entity (owner
// decision #020, a deviation from Plan/25 section 3.5, which also labelled bosses and the nearest monsters). `labelRole` decides who is a candidate
// and `selectLabels` places them; both are pure. `LabelLayer` keeps the DOM nodes (a pool: nodes are reused, never created per frame).
import { Kind } from './types';
import type { View } from './view';
import { worldToScreen } from './view';

export const MAX_LABELS = 150;
/** Labels collide on a grid of this many CSS pixels. */
export const LABEL_GRID_PX = 24;
/** Above this scale (world units per pixel) only the selection, the followed one and the acting character keep their label. */
export const HIDE_ABOVE_UPP = 4;

/** The rank of a label in the order of the plan: lower is shown first. */
export const LabelPriority = {
  SELECTED: 0,
  FOLLOWED: 1,
  ACTING: 2,
  PLAYER: 3,
  BOSS: 4,
  OTHER: 5,
} as const;

export interface LabelCandidate {
  readonly id: number;
  readonly text: string;
  /** The displayed world position. */
  readonly x: number;
  readonly z: number;
  readonly priority: number;
  /** Shown even above 4 u/px (the selection, the followed one, the acting character). */
  readonly always: boolean;
  /** A class for the colour (`player`, `npc`, `selected`; the engine no longer produces `monster` and `boss`). */
  readonly cls: string;
}

/** What decides whether an entity gets a label, and with which priority, whether it is always shown and which class it takes. */
export interface LabelRole {
  readonly priority: number;
  readonly always: boolean;
  readonly cls: string;
}

/**
 * The label role of one entity, or `null` when it gets no label (owner decision #020).
 *
 * A monster (every rank, with or without the BOSS flag, at any zoom) is labelled only when it is the ONLY selected entity or the followed one:
 * that label is the feedback of a click. A monster of a box or Shift selection gets none, so that selecting a field cannot bring the clutter
 * back. Every other kind keeps the plan's rules: the selection, the followed one, the acting character, players, and NPCs when `close`.
 */
export function labelRole(
  e: {
    readonly id: number;
    readonly kind: number;
    readonly sub: number;
    readonly flags: number;
  },
  s: {
    readonly selected: ReadonlySet<number>;
    readonly followId: number | null;
    readonly actingObjId: number | null;
    readonly close: boolean;
  },
): LabelRole | null {
  if (e.kind === Kind.MONSTER) {
    if (s.selected.size === 1 && s.selected.has(e.id))
      return { priority: LabelPriority.SELECTED, always: true, cls: 'selected' };
    if (e.id === s.followId)
      return { priority: LabelPriority.FOLLOWED, always: true, cls: 'selected' };
    return null;
  }
  if (s.selected.has(e.id))
    return { priority: LabelPriority.SELECTED, always: true, cls: 'selected' };
  if (e.id === s.followId)
    return { priority: LabelPriority.FOLLOWED, always: true, cls: 'selected' };
  if (e.id === s.actingObjId)
    return { priority: LabelPriority.ACTING, always: true, cls: 'player' };
  if (e.kind === Kind.PLAYER)
    return { priority: LabelPriority.PLAYER, always: false, cls: 'player' };
  if (s.close && e.kind === Kind.NPC)
    return { priority: LabelPriority.OTHER, always: false, cls: 'npc' };
  return null;
}

export interface PlacedLabel {
  readonly id: number;
  readonly text: string;
  readonly sx: number;
  readonly sy: number;
  readonly cls: string;
}

/** The estimated width of a label in CSS pixels (a 12 px UI font). */
export function labelWidth(text: string): number {
  return Math.min(220, text.length * 6.4 + 10);
}

/** Chooses and places the labels for a view: priority first, then the distance to the centre; a label whose grid cells are taken is skipped. */
export function selectLabels(
  cands: readonly LabelCandidate[],
  view: View,
  max = MAX_LABELS,
): PlacedLabel[] {
  const hide = view.upp > HIDE_ABOVE_UPP;
  const onScreen: { c: LabelCandidate; sx: number; sy: number; d: number }[] = [];
  for (const c of cands) {
    if (hide && !c.always) continue;
    const { sx, sy } = worldToScreen(view, c.x, c.z);
    if (sx < -40 || sx > view.w + 40 || sy < -20 || sy > view.h + 20) continue;
    const dx = sx - view.w / 2;
    const dy = sy - view.h / 2;
    onScreen.push({ c, sx, sy, d: dx * dx + dy * dy });
  }
  onScreen.sort((a, b) => a.c.priority - b.c.priority || a.d - b.d || a.c.id - b.c.id);
  const taken = new Set<number>();
  const out: PlacedLabel[] = [];
  for (const { c, sx, sy } of onScreen) {
    if (out.length >= max) break;
    const half = labelWidth(c.text) / 2;
    // the label sits below the marker: 8 px to 24 px under its centre
    const gx0 = Math.floor((sx - half) / LABEL_GRID_PX);
    const gx1 = Math.floor((sx + half) / LABEL_GRID_PX);
    const gy0 = Math.floor((sy + 8) / LABEL_GRID_PX);
    const gy1 = Math.floor((sy + 24) / LABEL_GRID_PX);
    let free = true;
    for (let gx = gx0; gx <= gx1 && free; gx += 1) {
      for (let gy = gy0; gy <= gy1; gy += 1) {
        if (taken.has(cell(gx, gy))) {
          free = false;
          break;
        }
      }
    }
    // the selection is never dropped for a collision
    if (!free && c.priority > LabelPriority.SELECTED) continue;
    for (let gx = gx0; gx <= gx1; gx += 1)
      for (let gy = gy0; gy <= gy1; gy += 1) taken.add(cell(gx, gy));
    out.push({ id: c.id, text: c.text, sx, sy, cls: c.cls });
  }
  return out;
}

function cell(gx: number, gy: number): number {
  return (gx + 512) * 4096 + (gy + 512);
}

/** The DOM side: a pool of absolutely positioned nodes inside `host`. */
export class LabelLayer {
  readonly #host: HTMLElement;
  readonly #nodes: HTMLDivElement[] = [];
  readonly #text: string[] = [];
  readonly #cls: string[] = [];
  #shown = 0;

  constructor(host: HTMLElement) {
    this.#host = host;
  }

  /** Labels currently shown. */
  get count(): number {
    return this.#shown;
  }

  update(placed: readonly PlacedLabel[]): void {
    for (let i = 0; i < placed.length; i += 1) {
      const p = placed[i];
      if (!p) continue;
      let node = this.#nodes[i];
      if (!node) {
        node = document.createElement('div');
        node.className = 'map-label';
        this.#host.appendChild(node);
        this.#nodes.push(node);
        this.#text.push('');
        this.#cls.push('');
      }
      if (this.#text[i] !== p.text) {
        node.textContent = p.text;
        this.#text[i] = p.text;
      }
      if (this.#cls[i] !== p.cls) {
        node.className = `map-label map-label--${p.cls}`;
        this.#cls[i] = p.cls;
      }
      if (i >= this.#shown) node.style.display = '';
      node.style.transform = `translate(${String(Math.round(p.sx))}px, ${String(Math.round(p.sy) + 8)}px) translateX(-50%)`;
    }
    for (let i = placed.length; i < this.#shown; i += 1) {
      const node = this.#nodes[i];
      if (node) node.style.display = 'none';
    }
    this.#shown = placed.length;
  }

  clear(): void {
    this.update([]);
  }

  dispose(): void {
    for (const n of this.#nodes) n.remove();
    this.#nodes.length = 0;
    this.#text.length = 0;
    this.#cls.length = 0;
    this.#shown = 0;
  }
}
