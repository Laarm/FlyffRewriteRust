// The rainbow slice of the events store (Plan/45 §28.0, §28.7; WP-CFEAT-44): the racer's progress as `RainbowRaceNowState` tells it (`CRainbowRace::GetInstance()`). Reset when the character leaves the world (a world
// change keeps it: the join batch resends `RainbowRaceNowState` for a racer in Progress, and `WndManager.cpp:2164` resets the remain time at the logout only).
//
//   nowGame        `m_wNowGame`: `1 << game` of the current mini-game, 0 for none
//   completed      `m_wGameState`: the finished games (bit per game, 0x7F = all seven)
//   endMs          `m_dwRemainTime`: set while the race runs (`remain_ms > 0` => `now + remain_ms`), absent otherwise; the client never expires it by the clock (KEEP)
//   checkEnd       `m_bCheckEnd`: an end box was closed since the race button was made; the all-done box waits for it
//   finishShown    `m_bRRFinishMsg`: the all-done box was shown once

export interface RainbowSlice {
  readonly nowGame: number;
  readonly completed: number;
  readonly endMs?: number;
  readonly checkEnd: boolean;
  readonly finishShown: boolean;
}

export function initialRainbow(): RainbowSlice {
  return { nowGame: 0, completed: 0, checkEnd: false, finishShown: false };
}

/** `m_dwRemainTime != 0`: the race is running for this racer. */
export function racingOf(s: RainbowSlice): boolean {
  return s.endMs !== undefined;
}
