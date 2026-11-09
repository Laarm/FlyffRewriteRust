// The stopwatch game (`CWndRRMiniGameStopWatch`, `src:_Interface/WndRainbowRace.cpp:1678-1847`, Plan/42 §19.4 P; WP-UI-62) as a pure model: the first press starts the clock, the second sends the time, a clock
// that runs past 60 s stops by itself and sends EXACTLY 60 000 once. The window shows `seconds mod 60` and the hundredths of the elapsed time and keeps the last stopped time.

/** `SEC( 60 )`: the clock stops when it runs past this. */
export const STOP_LIMIT_MS = 60_000;

const pad2 = (n: number): string => String(n).padStart(2, '0');

/** `SetTargetTime`: "SS : CC" of a target in ms (4570 -> "04 : 57"; the seconds wrap at 60). */
export function targetText(ms: number): string {
  const sec = Math.trunc(ms / 1000) % 60;
  const cs = Math.trunc((ms - Math.trunc(ms / 1000) * 1000) / 10);
  return `${pad2(sec)} : ${pad2(cs)}`;
}

/** The four digits of the display (`OnDraw`): the seconds mod 60 (tens, units) and the hundredths (tens, units). */
export interface StopwatchDigits {
  readonly sec1: number;
  readonly sec2: number;
  readonly cs1: number;
  readonly cs2: number;
}

export function digitsOf(ms: number): StopwatchDigits {
  const timeSec = Math.trunc(ms / 1000);
  const usec = Math.trunc(ms - timeSec * 1000);
  const sec = timeSec - Math.trunc(timeSec / 60) * 60;
  const cs = Math.trunc(usec / 10);
  return {
    sec1: Math.trunc(sec / 10),
    sec2: sec % 10,
    cs1: Math.trunc(cs / 10),
    cs2: cs % 10,
  };
}

/** What a press or a frame asks the window to do. */
export interface StopwatchStep {
  /** The time to send (`SendMinigamePacket( .., p1 )`), when the clock stopped. */
  readonly send?: number;
  /** The picture of the START button changed (`Buttstop.bmp` while running, `ButtStart.bmp` otherwise). */
  readonly running: boolean;
}

export class StopwatchModel {
  #running = false;
  #startAt = 0;
  /** The time the display shows (ms): the running clock, or the last stopped time. */
  #shown = 0;

  get running(): boolean {
    return this.#running;
  }
  get shownMs(): number {
    return this.#shown;
  }

  /** The START button: the first press starts, the second stops and sends the elapsed ms (`pinned` replaces them: the test hook). */
  press(now: number, pinned: number | null = null): StopwatchStep {
    if (!this.#running) {
      this.#startAt = now;
      this.#running = true;
      return { running: true };
    }
    this.#running = false;
    const elapsed = pinned ?? Math.max(0, Math.trunc(now - this.#startAt));
    this.#shown = elapsed;
    return { send: elapsed, running: false };
  }

  /** One frame: the running clock advances; past 60 s it stops and sends exactly 60 000. `pinned` replaces the elapsed time (the test hook). */
  process(now: number, pinned: number | null = null): StopwatchStep {
    if (!this.#running) return { running: false };
    const elapsed = pinned ?? Math.max(0, Math.trunc(now - this.#startAt));
    if (elapsed > STOP_LIMIT_MS) {
      this.#running = false;
      this.#shown = STOP_LIMIT_MS;
      return { send: STOP_LIMIT_MS, running: false };
    }
    this.#shown = elapsed;
    return { running: true };
  }
}
