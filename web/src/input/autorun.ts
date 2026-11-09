// Double-tap auto-run (Plan/43 §6.3; `WndWorldControlPlayer.cpp:148-200`, timers `WndWorld.cpp:630-632`): W, release within
// 200 ms, W again within 200 ms of the release and let go within 200 ms of that press switches auto-run on. S cancels it,
// and so does pressing W again 200 ms or more after the auto-run started (or while still holding the second press).
//
// Evaluated once per control tick that runs the ground control; the three timers are `CTimer`s of 200 ms whose `Reset()`
// stores the time and whose `TimeOut()` is `now - t >= 200`. A fresh timer is expired.

export const AUTORUN_PUSH_MS = 200;
export const AUTORUN_DOUBLE_MS = 200;
export const AUTORUN_BLOCK_MS = 200;

/** `m_nDubleUp`: 0 idle, 1 first press seen, 2 released after the first press, 3 auto-run started. */
export type AutoRunPhase = 0 | 1 | 2 | 3;

export class AutoRun {
  n: AutoRunPhase = 0;
  /** `g_WndMng.m_bAutoRun`. */
  active = false;
  private tPush = Number.NEGATIVE_INFINITY;
  private tAuto = Number.NEGATIVE_INFINITY;
  private tBlock = Number.NEGATIVE_INFINITY;

  /**
   * One evaluation. `up` / `down` are the held W and S (after the chat-focus clearing). Returns the effective `up`: W held,
   * or auto-run active.
   */
  step(up: boolean, down: boolean, now: number): boolean {
    if (down) this.active = false;
    if (up) {
      this.tPush = now;
      if (this.n === 2 && now - this.tAuto < AUTORUN_DOUBLE_MS) {
        this.n = 3;
        this.active = true;
        this.tAuto = now;
        this.tBlock = now;
      } else {
        this.n = 1;
        this.tAuto = now;
      }
      if (now - this.tBlock >= AUTORUN_BLOCK_MS) this.active = false;
    } else if (now - this.tPush < AUTORUN_PUSH_MS) {
      if (this.n === 1) this.n = 2;
    } else {
      this.n = 0;
    }
    return up || this.active;
  }

  /** `OpenField` (world entry), S, a world change: auto-run ends and the phase restarts. */
  reset(): void {
    this.n = 0;
    this.active = false;
    this.tPush = Number.NEGATIVE_INFINITY;
    this.tAuto = Number.NEGATIVE_INFINITY;
    this.tBlock = Number.NEGATIVE_INFINITY;
  }
}
