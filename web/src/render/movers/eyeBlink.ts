// Eye blink of players (Plan/41 section 12.5, WP-REND-05; `CMover::ProcessEyeFlash`, Mover.cpp:10916-10972).
//
// Within 11 m of the camera the eyes follow two millisecond timers: when timer 0 expires the closed texture
// (`_Flash`) is shown and the timers become (1050, 50); when timer 1 expires the open texture is shown for `s`
// seconds, `s = 5 + [0, 2)` (10 %: 0.5 s), i.e. timer 1 = `s + 1000`, timer 0 = `s`: closed for 50 ms, then open for
// 5-7 s. Dead players always have closed eyes; beyond 11 m the eyes are open (closed when dead) and the timers do
// not run. The texture swap targets GMOBJECT 0, material block 0 of the head part of THIS instance (FIX of the
// original, which swapped the shared object, PL-137).
//
// The timers are per mover and use the `nowMs` of the frame (`CTimer` of the original ran on `timeGetTime()`); a
// fresh `EyeBlink` starts with both timers expired (`CTimer` constructed with a zero interval), so the first
// update inside 11 m starts with a blink.

/** The distance up to which the blink runs (`m_fDistCamera <= 11`). */
export const BLINK_DISTANCE = 11;
/** `fSec` of the closed phase. */
export const BLINK_CLOSED_MS = 50;

/** A `CTimer`: `set(ms)` starts it, `expired()` is `now - start >= interval`. */
class Timer {
  private start = 0;
  private interval = 0;
  set(now: number, ms: number): void {
    this.start = now;
    this.interval = ms;
  }
  expired(now: number): boolean {
    return now - this.start >= this.interval;
  }
}

export class EyeBlink {
  private readonly t0 = new Timer();
  private readonly t1 = new Timer();
  private closedNow = false;

  /** @param rand a function returning a number in `[0, 1)` (`xRandomF`, `xRandom`); per mover. */
  constructor(private readonly rand: () => number) {}

  /** `true` while the closed-eyes texture is shown. */
  get closed(): boolean {
    return this.closedNow;
  }

  /**
   * One `ProcessEyeFlash` step. `distCamera` is the distance to the camera, `dead` the death flag of the render
   * view. Returns `closed`.
   */
  update(nowMs: number, distCamera: number, dead: boolean): boolean {
    if (dead) {
      this.closedNow = true;
      return true;
    }
    if (distCamera <= BLINK_DISTANCE) {
      if (this.t0.expired(nowMs)) {
        this.closedNow = true;
        this.t0.set(nowMs, BLINK_CLOSED_MS + 1000);
        this.t1.set(nowMs, BLINK_CLOSED_MS);
      }
      if (this.t1.expired(nowMs)) {
        // the eyes are open for 5-7 s, once in ten times only 0.5 s
        let sec = (this.rand() * 2 + 5) * 1000;
        if (Math.floor(this.rand() * 100) < 10) sec = 500;
        this.closedNow = false;
        this.t1.set(nowMs, sec + 1000);
        this.t0.set(nowMs, sec);
      }
    } else {
      this.closedNow = false;
    }
    return this.closedNow;
  }
}
