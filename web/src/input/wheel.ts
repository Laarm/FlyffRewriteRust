// Wheel normalisation (Plan/43 §4.9, PL-164; `CWndWorld::OnMouseWheel`). The original changed the zoom by one step per
// `WM_MOUSEWHEEL` message (`zDelta = +-120`). Browsers deliver heterogeneous deltas, so every event is normalised to
// notches: one notch = one 120-unit message, one zoom step of 0.5.
//
//   legacy `wheelDeltaY` a non-zero multiple of 120 (most mice, all engines):      n = -wheelDeltaY / 120
//   `DOM_DELTA_LINE`  (Firefox, 3 lines per notch):                              acc += deltaY / 3
//   `DOM_DELTA_PAGE`:                                                            acc += deltaY
//   `DOM_DELTA_PIXEL`: a discrete wheel (|deltaY| >= 40) is one notch per event, a smaller delta (touchpads,
//                      high-resolution wheels) accumulates deltaY / 100
//   the whole part of `acc` is the notch count, the remainder is kept; it is dropped after 200 ms without an event.
//
// Positive = the wheel moved towards the user (scroll down), the original's `zDelta < 0`.

export const WHEEL_LINES_PER_NOTCH = 3;
export const WHEEL_PX_PER_NOTCH = 100;
export const WHEEL_PX_DISCRETE = 40;
export const WHEEL_IDLE_RESET_MS = 200;

export const DOM_DELTA_PIXEL = 0;
export const DOM_DELTA_LINE = 1;
export const DOM_DELTA_PAGE = 2;

/** The fields of a `WheelEvent` the normaliser reads. */
export interface WheelEventLike {
  readonly deltaY: number;
  readonly deltaMode: number;
  /** The legacy `wheelDeltaY` (absent or 0 when the engine does not provide it). */
  readonly wheelDeltaY?: number;
}

/** Remainders that float rounding leaves just below a whole notch (ten events of 0.1 sum to 0.999...). */
const EPS = 1e-9;

export class WheelNormalizer {
  private acc = 0;
  private lastAt = Number.NEGATIVE_INFINITY;

  /** The notches of one event (an integer, signed); `nowMs` is the event time. */
  feed(e: WheelEventLike, nowMs: number): number {
    if (nowMs - this.lastAt > WHEEL_IDLE_RESET_MS) this.acc = 0;
    this.lastAt = nowMs;
    const legacy = e.wheelDeltaY ?? 0;
    if (legacy !== 0 && legacy % 120 === 0) {
      this.acc = 0;
      return -legacy / 120;
    }
    if (e.deltaMode === DOM_DELTA_LINE) {
      this.acc += e.deltaY / WHEEL_LINES_PER_NOTCH;
    } else if (e.deltaMode === DOM_DELTA_PAGE) {
      this.acc += e.deltaY;
    } else if (Math.abs(e.deltaY) >= WHEEL_PX_DISCRETE) {
      this.acc += Math.sign(e.deltaY);
    } else {
      this.acc += e.deltaY / WHEEL_PX_PER_NOTCH;
    }
    const n = Math.trunc(this.acc + (this.acc >= 0 ? EPS : -EPS));
    this.acc -= n;
    return n === 0 ? 0 : n;
  }

  reset(): void {
    this.acc = 0;
    this.lastAt = Number.NEGATIVE_INFINITY;
  }
}
