// The ladder game (`CWndRRMiniGameLadder`, `src:_Interface/WndRainbowRace.cpp:2192-2750`, Plan/42 §19.4 P; WP-UI-62) as pure functions and one frame stepper: the random table of bars (`MakeLadder`, exactly the
// draws of the original, including its dead `if( i == 0 )` branch for the last column: KEEP, cosmetic), the goal and the failing target, and the run of the ball (3 px down and 2 px sideways per 60 Hz frame
// through the levels `bg.top + 20 + 25 ( k + 1 )`) with the trail rectangles it leaves.

import type { Rect } from '../../layout/geometry';
import type { Rng } from './rng';

export const LADDER_COLS = 15;
export const LADDER_ROWS = 10;

/** `GO_DOWN`, `GO_LEFT`, `GO_RIGHT`. */
const GO_DOWN = 0;
const GO_LEFT = 1;
const GO_RIGHT = 2;

/** The result of `MakeLadder`. */
export interface MadeLadder {
  /** `m_nLadder[ column ][ row ]`: the column the bar leads to, -1 for none. */
  readonly ladder: number[][];
  /** The column the ball ends in when it starts at `start`. */
  readonly goal: number;
  /** The column the goal box is drawn at: the goal on FINISH, a random other column on FAIL. */
  readonly target: number;
}

/** A table of -1 (`Reset`). */
export function emptyLadder(): number[][] {
  return Array.from({ length: LADDER_COLS }, () => new Array<number>(LADDER_ROWS).fill(-1));
}

/** The column of a start: follows the start column down the ten rows. */
export function followLadder(ladder: readonly (readonly number[])[], start: number): number {
  let goal = start;
  for (let row = 0; row < LADDER_ROWS; row += 1) {
    const next = ladder[goal]?.[row] ?? -1;
    if (next !== -1) goal = next;
  }
  return goal;
}

/** `MakeLadder`: `finish` selects the goal (FINISH) or a random other column (FAIL) as the target. */
export function makeLadder(rng: Rng, start: number, finish: boolean): MadeLadder {
  const ladder = emptyLadder();
  const at = (c: number, r: number): number => ladder[c]?.[r] ?? -1;
  const set = (c: number, r: number, v: number): void => {
    const col = ladder[c];
    if (col) col[r] = v;
  };
  for (let i = 0; i < LADDER_COLS; i += 1) {
    for (let j = 0; j < LADDER_ROWS; j += 1) {
      if (i > 0 && i < LADDER_COLS - 1) {
        let next = rng(3);
        if (at(i - 1, j) !== i) {
          if (i > 1 && at(i - 2, j) === i - 1 && next === GO_LEFT) next = GO_DOWN;
          if (next === GO_DOWN) set(i, j, -1);
          else if (next === GO_LEFT) {
            set(i, j, i - 1);
            set(i - 1, j, i);
          } else {
            set(i, j, i + 1);
            set(i + 1, j, i);
          }
        }
      } else if (i === 0) {
        if (rng(2) === 0) {
          set(0, j, 1);
          set(1, j, 0);
        } else set(0, j, -1);
      } else if (at(i - 1, j) === i) set(i, j, i - 1);
      else if (at(i - 2, j) === i - 1) set(i, j, -1);
      else {
        // the original draws and then does nothing in either case (its `if( i == 0 )` is dead here)
        if (rng(2) !== 0) set(i, j, -1);
      }
    }
  }
  const goal = followLadder(ladder, start);
  let other = goal;
  // `while( nTarget == nGoal ) nTarget = rand() % 15`; a degenerate source cannot hang the window: after 64 equal draws the next column stands in
  for (let tries = 0; other === goal; tries += 1) {
    other = tries < 64 ? rng(LADDER_COLS) : (goal + 1) % LADDER_COLS;
  }
  return { ladder, goal, target: finish ? goal : other };
}

/** The numbers of the window the run needs. */
export interface LadderGeometry {
  /** `rect.left` of the start cell of each column. */
  readonly startLeft: readonly number[];
  /** `rect.top` of the start cells. */
  readonly startTop: number;
  /** The top and the bottom of `WIDC_CUSTOM_BG`. */
  readonly bgTop: number;
  readonly bgBottom: number;
}

const copy = (r: Rect): Rect => ({ l: r.l, t: r.t, r: r.r, b: r.b });

/** The ball and its trail (`m_ptPoint`, `m_rectPointLine`, `m_rectLine`, `m_nLevel`, `m_nGoTo`, `Process`). */
export class LadderRun {
  readonly lines: Rect[] = [];
  pt: { x: number; y: number };
  /** The trail rectangle being drawn (`m_rectPointLine`). */
  line: Rect;
  level = -1;
  point: number;
  goTo = GO_DOWN;
  readonly #geom: LadderGeometry;
  readonly #ladder: readonly (readonly number[])[];
  readonly #start: number;

  constructor(geom: LadderGeometry, ladder: readonly (readonly number[])[], start: number) {
    this.#geom = geom;
    this.#ladder = ladder;
    this.#start = start;
    this.point = start;
    this.pt = { x: geom.startLeft[start] ?? 0, y: geom.startTop };
    this.line = {
      l: this.pt.x + 9,
      r: this.pt.x + 9 + 5,
      t: this.pt.y + 24,
      b: this.pt.y + 24,
    };
  }

  /** One 60 Hz frame; `true` when the ball has reached the bottom (`m_nLevel == 10 && bEnd`). */
  step(): boolean {
    const down = 3;
    const turn = 2;
    const g = this.#geom;
    if (this.level >= -1 && this.level < 10) {
      const firstTop = g.bgTop + 20;
      if (this.level === -1) this.point = this.#start;
      const startLeft = g.startLeft[this.point] ?? this.pt.x;
      if (this.goTo === GO_DOWN) {
        const nextY = this.level === -1 ? firstTop : firstTop + 25 * (this.level + 1);
        if (this.pt.y < nextY) {
          this.pt.y += down;
          this.line.b = this.pt.y;
        } else {
          this.line.b += 10;
          this.lines.push(copy(this.line));
          this.level += 1;
          if (this.level < 10) {
            const next = this.#ladder[this.point]?.[this.level] ?? -1;
            if (next === -1) {
              this.goTo = GO_DOWN;
              this.line.t = this.line.b;
            } else if (next < this.point) {
              this.point = next;
              this.goTo = GO_LEFT;
              this.line.t = this.line.b - 2;
              this.line.b += 3;
              this.line.l = this.line.r;
            } else if (next > this.point) {
              this.point = next;
              this.goTo = GO_RIGHT;
              this.line.t = this.line.b - 2;
              this.line.b += 3;
              this.line.r = this.line.l;
            }
          }
        }
      } else if (this.goTo === GO_LEFT) {
        if (this.pt.x > startLeft) {
          this.pt.x -= turn;
          this.line.l = this.pt.x;
        } else {
          this.goTo = GO_DOWN;
          this.line.l += 9;
          this.lines.push(copy(this.line));
          this.line.r = this.line.l + 5;
        }
      } else if (this.pt.x < startLeft) {
        this.pt.x += turn;
        this.line.r = this.pt.x;
      } else {
        this.goTo = GO_DOWN;
        this.line.r += 14;
        this.lines.push(copy(this.line));
        this.line.l = this.line.r - 5;
      }
      return false;
    }
    if (this.level === 10) {
      if (this.pt.y < g.bgBottom) {
        this.pt.y += down;
        this.line.b = this.pt.y;
        return false;
      }
      return true;
    }
    return false;
  }
}

/** The lerp of the pulsing trail and frame (`SetFillRectValue` / `FillRect`): a step per drawn frame, bouncing between 0 and 1. */
export class Pulse {
  lerp = 0;
  #reverse = false;
  readonly #step: number;

  constructor(step: number) {
    this.#step = step;
  }

  /** One drawn frame. */
  tick(): number {
    this.lerp += this.#reverse ? -this.#step : this.#step;
    if (this.lerp > 1) {
      this.#reverse = true;
      this.lerp = 1;
    } else if (this.lerp < 0) {
      this.#reverse = false;
      this.lerp = 0;
    }
    return this.lerp;
  }
}
