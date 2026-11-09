// The 60 Hz frame counter of the mini-game windows. The C++ `Process()` of these windows runs once per rendered frame at `FRAME_PER_SEC` 60 and counts frames (the spin of the hands, the
// dice); the shells are processed once per UI tick whatever the display rate, so a 120 Hz display must not double the speed: `advance( now )` answers the whole frames elapsed since the
// last call (the remainder is kept). The first call only starts the clock. A jump of more than `MAX_CATCH_UP` frames (the tab was hidden, a fake clock leapt) is not replayed, it ends the
// round of counting where it stands. `WndPetMiracle` counts calls instead (its NOTES line); the mini-games count time.

import { FRAME_MS } from '../../../platform/time';

/** The most frames one `advance` hands out (10 s). */
export const MAX_CATCH_UP = 600;

export class FrameClock {
  #last: number | null = null;
  #rest = 0;

  /** The whole 60 Hz frames since the previous call. */
  advance(now: number): number {
    if (this.#last === null) {
      this.#last = now;
      return 0;
    }
    const dt = now - this.#last;
    this.#last = now;
    if (!(dt > 0)) return 0;
    const total = this.#rest + dt;
    // a tiny tolerance: 1000 / 60 steps accumulate float error (59.9999 frames in a second)
    const frames = Math.floor(total / FRAME_MS + 1e-6);
    this.#rest = Math.max(0, total - frames * FRAME_MS);
    return Math.min(frames, MAX_CATCH_UP);
  }
}
