// The scene of one minimap frame (`CWndNavigator::OnEraseBkgnd` + `RenderMarkAll`; ref/99 §4.4, Plan/45 §7.6; WP-CFEAT-28, -35): everything
// the painter draws, computed from plain inputs, so the tests can assert the numbers (which lands, which frame for which mover, where a
// mark sits) without a canvas.

import { type Dot, classify, dotBox, dotHit, type MoverSample, type NavEnv } from './dots';
import {
  MARK_HIT,
  markFrame,
  markPosition,
  type TileRect,
  type ViewSize,
  visibleTiles,
  worldToView,
} from './geometry';

/** A mark of the party or of the own character (`m_nvPoint`, `m_vOtherPoint`). */
export interface MarkInput {
  readonly x: number;
  readonly z: number;
  /** The name that the tooltip shows (the sender; the own mark shows the own name). */
  readonly name: string;
  readonly self: boolean;
}

export interface NavInput {
  /** `m_size.cx`: the pixel size of one land. */
  readonly size: number;
  /** The client size of the window. */
  readonly view: ViewSize;
  /** The player: the view is centred on it. */
  readonly px: number;
  readonly pz: number;
  /** Degrees clockwise from north that the arrow points to. */
  readonly heading: number;
  /** Lands in x and z. */
  readonly lands: readonly [number, number];
  /** An indoor world or an instance dungeon: the fog picture, no objects. */
  readonly fogged: boolean;
  readonly movers: readonly MoverSample[];
  readonly env: NavEnv;
  /** The focused object (`GetObjFocus()`), `null` for none. */
  readonly focus: { readonly x: number; readonly z: number } | null;
  /** The marks that are still drawn (own + party). */
  readonly marks: readonly MarkInput[];
  /** The destination arrow (`m_vDestinationArrow`, V15 `__IMPROVE_QUEST_INTERFACE`), `null` for none (optional: absent = none). */
  readonly destination?: { readonly x: number; readonly z: number } | null;
  readonly nowMs: number;
}

export interface SceneDot {
  readonly dot: Dot;
  /** The centre point in view pixels. */
  readonly x: number;
  readonly y: number;
}

export interface SceneMark {
  readonly name: string;
  readonly self: boolean;
  /** The point the 16 x 16 sprite is centred on. */
  readonly x: number;
  readonly y: number;
}

export interface NavScene {
  readonly size: number;
  readonly view: ViewSize;
  readonly tiles: readonly TileRect[];
  readonly fogged: boolean;
  /** Dots and structure icons in the order of the legacy loop. */
  readonly dots: readonly SceneDot[];
  /** The quest NPCs, drawn after the others. */
  readonly quests: readonly SceneDot[];
  /** The focus marker (frame 5, 7 x 7 offset 3). */
  readonly focus: { readonly x: number; readonly y: number } | null;
  readonly marks: readonly SceneMark[];
  /** The destination sprite's point (`ButtDestination.bmp` is drawn scaled 0.4 at the point minus 3; none in a fogged world), `null` for none. */
  readonly destination: { readonly x: number; readonly y: number } | null;
  /** `m_iFrame` of `NavPosition.inc`. */
  readonly markFrame: number;
  readonly heading: number;
}

/** The dots of the movers that the filters let through, in input order. */
export function buildDots(i: NavInput): { dots: SceneDot[]; quests: SceneDot[] } {
  const dots: SceneDot[] = [];
  const quests: SceneDot[] = [];
  let count = 0;
  for (const m of i.movers) {
    const d = classify(m, i.env);
    if (!d) continue;
    const at = worldToView(d.x, d.z, i.px, i.pz, i.size, i.view);
    if (d.kind === 'quest') {
      quests.push({ dot: d, x: at.x, y: at.y });
      continue;
    }
    // the legacy buffer holds 1000 vertices
    if (count >= 1000) continue;
    count += 1;
    dots.push({ dot: d, x: at.x, y: at.y });
  }
  return { dots, quests };
}

export function buildScene(i: NavInput): NavScene {
  const tiles = visibleTiles(i.lands, i.px, i.pz, i.size, i.view);
  const { dots, quests } = i.fogged ? { dots: [], quests: [] } : buildDots(i);
  const focus =
    !i.fogged && i.focus ? worldToView(i.focus.x, i.focus.z, i.px, i.pz, i.size, i.view) : null;
  const destination =
    !i.fogged && i.destination
      ? worldToView(i.destination.x, i.destination.z, i.px, i.pz, i.size, i.view)
      : null;
  const marks: SceneMark[] = i.marks.map((m) => {
    const at = markPosition(m.x, m.z, i.px, i.pz, i.size, i.view);
    return { name: m.name, self: m.self, x: at.x, y: at.y };
  });
  return {
    size: i.size,
    view: i.view,
    tiles,
    fogged: i.fogged,
    dots,
    quests,
    focus,
    marks,
    destination,
    markFrame: markFrame(i.nowMs),
    heading: i.heading,
  };
}

/** The dot under a client point: the LAST one in draw order (each hit overwrote the tooltip in the legacy loop). */
export function dotAt(scene: NavScene, x: number, y: number): SceneDot | null {
  let hit: SceneDot | null = null;
  for (const d of scene.dots) if (dotHit(d.x, d.y, x, y)) hit = d;
  return hit;
}

/** The mark under a client point (`CRect( point - 8, point + 8 )`). */
export function markAt(scene: NavScene, x: number, y: number): SceneMark | null {
  let hit: SceneMark | null = null;
  for (const m of scene.marks) {
    if (x >= m.x - MARK_HIT && x < m.x + MARK_HIT && y >= m.y - MARK_HIT && y < m.y + MARK_HIT) {
      hit = m;
    }
  }
  return hit;
}

/** The rect (client coordinates) that a dot occupies, for the tooltip's owner rect. */
export function dotRect(d: SceneDot): { l: number; t: number; r: number; b: number } {
  const b = dotBox(d.dot);
  return { l: d.x + b.dx, t: d.y + b.dy, r: d.x + b.dx + b.size, b: d.y + b.dy + b.size };
}

/**
 * `OnLButtonDown`'s selection: the first mover (not transparent, not the player) whose 5 x 5 box around the point holds the click. The
 * place filters do NOT apply here and game masters are selectable (the loop tests `TRANSPARENT_MODE` only).
 */
export function pickMover(
  movers: readonly MoverSample[],
  selfId: number,
  px: number,
  pz: number,
  size: number,
  view: ViewSize,
  x: number,
  y: number,
): MoverSample | null {
  for (const m of movers) {
    if (m.id === selfId || m.transparent) continue;
    const at = worldToView(m.x, m.z, px, pz, size, view);
    if (dotHit(at.x, at.y, x, y)) return m;
  }
  return null;
}
