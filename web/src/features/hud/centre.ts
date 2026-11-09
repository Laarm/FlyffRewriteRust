// Centre client messages (Plan/45 §7.11; `CClientMsg`, ClientMsg.cpp, drawn by `CNeuzApp::Render` at `(w / 2, 520 * h / 768)` in
// `FontWorld`): the lines the chat router sends to the CLIENT target (game messages by default). At most six lines; a new
// line restarts a 3 000 ms timer; when the timer runs out the OLDEST line starts to fade (alpha 254) and the timer restarts;
// every fading line loses 10 alpha per frame and the oldest is removed when one goes below 0. So lines vanish one by one, a
// quiet second line after the first one.

/** `nMax` of `AddMessage`. */
export const CENTRE_MAX_LINES = 6;
/** `m_timer.Set(3000)`. */
export const CENTRE_HOLD_MS = 3000;
/** The alpha lost per frame by a fading line. */
export const CENTRE_FADE_STEP = 10;

export interface CentreLine {
  readonly text: string;
  /** `0xAARRGGBB` colour of the message (the alpha byte is replaced by `alpha`). */
  readonly color: number;
  /** 255 = fresh, below = fading. */
  alpha: number;
}

export class CentreMessages {
  readonly lines: CentreLine[] = [];
  #timerEnd = 0;

  /** `AddMessage`: appends, drops the oldest above six and restarts the hold timer. */
  add(text: string, color: number, nowMs: number): void {
    this.lines.push({ text, color: color >>> 0, alpha: 255 });
    if (this.lines.length > CENTRE_MAX_LINES) this.lines.shift();
    this.#timerEnd = nowMs + CENTRE_HOLD_MS;
  }

  clear(): void {
    this.lines.length = 0;
  }

  /**
   * Drops every line whose text is `text` and returns how many went. The original never removes a single line (the hold timer
   * and the fade do); the client does for a message whose cause is gone, such as the death line after the revival.
   */
  remove(text: string): number {
    let removed = 0;
    for (let i = this.lines.length - 1; i >= 0; i -= 1) {
      if (this.lines[i]?.text === text) {
        this.lines.splice(i, 1);
        removed += 1;
      }
    }
    return removed;
  }

  /**
   * One rendered frame of the original: fading lines lose 10 alpha (the oldest goes when one is below 0), then an expired
   * timer starts the fade of the oldest line and restarts the timer. `true` when something changed.
   */
  frame(nowMs: number): boolean {
    let changed = false;
    for (let i = this.lines.length - 1; i >= 0; i -= 1) {
      const line = this.lines[i];
      if (!line) continue;
      if (line.alpha !== 255) {
        line.alpha -= CENTRE_FADE_STEP;
        changed = true;
        if (line.alpha < 0) {
          this.lines.shift();
          // the array shifted down by one under the unvisited lines: the original decrements the index to follow it
          i -= 1;
        }
      }
    }
    if (nowMs > this.#timerEnd && this.lines.length > 0) {
      const first = this.lines[0];
      this.#timerEnd = nowMs + CENTRE_HOLD_MS;
      if (first?.alpha === 255) {
        first.alpha = 254;
        changed = true;
      }
    }
    return changed;
  }
}
