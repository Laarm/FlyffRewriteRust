// The Tab key on the siege map (Plan/43 §5.2, §7.11, §10.2; WP-INPUT-16; KQ-31, PL-165): `CWorld::Render` collected up to `MAX_MOVERSELECT` enemy
// players around the player into `m_amvrSelect` only while the world is `WI_WORLD_GUILDWAR`, and `CWndWorld::OnKeyUp(VK_TAB)` cycled through them with
// the loop and the static counter below. Everywhere else the list is empty: Tab does nothing, the counter still moves.
//
// The list (here built ON the key-up from the movers drawn in the last frame): a player, not the own one, not dead, not `TRANSPARENT`, whose guild is
// known and differs from the own known guild, within `IsRangeObj( selfPos, 20 )` - `|self - cand|^2 < (0.8 + 0.8 * radius + 20)^2` in single
// precision, the formula of the sim's `range_point` (`gwTab.wasm.test.ts` compares the two). DEVIATIONS: the list is sorted by ascending ObjId before the
// cap of five (PL-165; the original kept the draw order), and the range test runs in TS because the query cap Q is two per iteration (Plan/43 §10.2).
//
// The cycle is the original loop VERBATIM, including its quirk (KQ-31): the counter starts at 0, lives for the whole session (a world change does not
// reset it) and ends at the index of the pick, so with two or more candidates Tab alternates between the first two and never reaches the third; with one
// candidate it alternates "nothing", E0, "nothing"; with none the counter goes 0 -> 5 and stays.

import type { Vec3LH } from '../render/types';

/** `MAX_MOVERSELECT`: the size of the list and the end of the cycle. */
export const MAX_GW_TARGETS = 5;
/** `IsRangeObj( g_pPlayer->GetPos(), 20.0f )`: the range in metres. */
export const GW_RANGE = 20;

const f32 = Math.fround;
/** `0.8f` (the C++ literals of `IsRangeObj`). */
const K = f32(0.8);

/** One mover the list may take (the facts of `entityInfo`). */
export interface GwCandidate {
  readonly id: number;
  readonly isPlayer: boolean;
  readonly dead: boolean;
  readonly transparent: boolean;
  /** `visual.guild.idGuild`, 0 when the guild is not known. */
  readonly guildId: number;
  readonly pos: Vec3LH | null;
  /** `GetRadius()`. */
  readonly radius: number;
}

/**
 * `bounds::is_range_point( pos, radius, point, range )` in single precision: `R = 0.8 + (0.8 * radius + range)`, true iff `|point - pos|^2 < R^2`.
 * The candidate's own radius counts, the point (the player's position) has none.
 */
export function inRangeOfPoint(pos: Vec3LH, radius: number, point: Vec3LH, range: number): boolean {
  const rSrc = f32(f32(f32(radius) * K) + f32(range));
  const bigR = f32(f32(f32(1) * K) + rSrc);
  const dx = f32(f32(point[0]) - f32(pos[0]));
  const dy = f32(f32(point[1]) - f32(pos[1]));
  const dz = f32(f32(point[2]) - f32(pos[2]));
  const lenSq = f32(f32(f32(dx * dx) + f32(dy * dy)) + f32(dz * dz));
  return lenSq < f32(bigR * bigR);
}

/** The own player as the list needs it. */
export interface GwSelf {
  readonly id: number;
  readonly guildId: number;
  readonly pos: Vec3LH;
}

/**
 * The Tab list: the candidates that pass every filter, ascending ObjId, at most {@link MAX_GW_TARGETS}. The own guild must be known (a player without a
 * guild has no enemies: `if( pGuild1 && pGuild2 )`).
 */
export function buildGuildWarList(
  candidates: Iterable<GwCandidate>,
  self: GwSelf,
  out: number[] = [],
): number[] {
  out.length = 0;
  if (self.guildId === 0) return out;
  const hits: number[] = [];
  for (const c of candidates) {
    if (c.id === self.id || !c.isPlayer || c.dead || c.transparent) continue;
    if (c.guildId === 0 || c.guildId === self.guildId) continue;
    if (c.pos === null || !inRangeOfPoint(c.pos, c.radius, self.pos, GW_RANGE)) continue;
    hits.push(c.id);
  }
  hits.sort((a, b) => a - b);
  for (let i = 0; i < hits.length && i < MAX_GW_TARGETS; i += 1) out.push(hits[i] ?? 0);
  return out;
}

/** What the last key-up did (the probe `input.gwTab()` and the tests read it). */
export interface GwTabSnapshot {
  /** The cycle counter now (`n_nMoverSelectCount`). */
  count: number;
  /** The list of the last key-up. */
  list: number[];
  /** The object the last key-up focused (`null`: nothing). */
  last: number | null;
  /** Key-ups handled. */
  presses: number;
}

/** The ports of one key-up: the world is the siege map, the candidates drawn in the last frame, the checks and the focus. */
export interface GwTabEnv {
  guildWarMap(): boolean;
  self(): GwSelf | null;
  candidates(): Iterable<GwCandidate>;
  /** The mover is neither `TRANSPARENT` nor dead right now. */
  focusable(id: number): boolean;
  setFocus(id: number): void;
}

/** The cycle state (`n_nMoverSelectCount`) and the list of the last key-up. */
export class GuildWarTab {
  /** `n_nMoverSelectCount`: starts at 0, lives for the session, is NOT reset by a world change. */
  count = 0;
  /** The list of the last key-up. */
  readonly list: number[] = [];
  last: number | null = null;
  presses = 0;

  /**
   * The loop of `CWndWorld::OnKeyUp( VK_TAB )` over `list`: returns the index to focus (`i`) and the mover or `null`; the counter becomes `i`.
   * `if( count > MAX ) count = 0; for( ; i < MAX; i++ ) { if( i == count ) continue; m = list[i]; if( m ) break; }`
   */
  cycle(list: readonly number[], focusable: (id: number) => boolean): number | null {
    if (this.count > MAX_GW_TARGETS) this.count = 0;
    let i = 0;
    let m: number | null = null;
    for (; i < MAX_GW_TARGETS; i += 1) {
      if (i === this.count) continue;
      m = list[i] ?? null;
      if (m !== null) break;
    }
    // (no break: the last assignment was a null entry, `pMover` is NULL)
    const pick = m !== null && focusable(m) ? m : null;
    this.count = i;
    return pick;
  }

  /** One Tab key-up: builds the list (empty off the siege map), runs the cycle and focuses the pick. */
  press(env: GwTabEnv): void {
    this.presses += 1;
    const self = env.self();
    this.list.length = 0;
    if (env.guildWarMap() && self !== null) buildGuildWarList(env.candidates(), self, this.list);
    const pick = this.cycle(this.list, (id) => env.focusable(id));
    this.last = pick;
    if (pick !== null) env.setFocus(pick);
  }

  snapshot(): GwTabSnapshot {
    return { count: this.count, list: [...this.list], last: this.last, presses: this.presses };
  }
}
