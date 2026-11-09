// The pending motion chat (`m_szTimerChat` / `m_timerChat`, `src:_Interface/WndManager.cpp:3007-3013, 3532-3537`; Plan/45 §19.4): the
// name of an executed motion ("Rock", "North") is sent as a chat line 1000 ms later; a second motion within that second replaces
// the text and restarts the second.

/** `m_timerChat.Set(1000)`. */
export const MOTION_CHAT_MS = 1000;

export class PendingChat {
  #text = '';
  #due = 0;

  /** The pending text, `''` when none. */
  get text(): string {
    return this.#text;
  }

  /** Stores `text` and restarts the timer. */
  set(text: string, nowMs: number): void {
    this.#text = text;
    this.#due = nowMs + MOTION_CHAT_MS;
  }

  /** The text when its second is over (it is cleared), else `null`. */
  take(nowMs: number): string | null {
    if (this.#text === '' || nowMs < this.#due) return null;
    const t = this.#text;
    this.#text = '';
    this.#due = 0;
    return t;
  }
}
