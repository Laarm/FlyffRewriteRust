// The spin of the rock-paper-scissors game (`CWndRRMiniGameKawiBawiBo::Process`, `src:_Interface/WndRainbowRace.cpp:1004-1085`, Plan/42 §19.4 P; WP-UI-61) as a pure state machine over 60 Hz frames: the computer's
// hand turns, slows down once the server's answer is in and stops on the hand the answer calls for. The counters are the original's, including the stale `count` that the next spin starts with (cosmetic, KEEP).
//
//   status 0   idle                 status 1   the hand turns (a step every `delay + 1` frames; `delay` grows to 20 once the result is in)
//   status 2   the result stands for 40 frames, then TRUE / FINISH count a win, FAIL resets the streak (and opens the DEFEAT box)

import { MP } from '../constants';

/** `KAWI`, `BAWI`, `BO`. */
export const KAWI = 0;
export const BAWI = 1;
export const BO = 2;

/** `GetKawiBawiBoComResult`: the computer's hand for the result of the server's roll. */
export function comHand(my: number, result: number): number {
  if (result === MP.TRUE || result === MP.FINISH)
    return my === KAWI ? BO : my === BAWI ? KAWI : BAWI;
  if (result === MP.FAIL) return my === KAWI ? BAWI : my === BAWI ? BO : KAWI;
  if (result === MP.FALSE) return my;
  return 0;
}

/** What a frame asks the window to do. */
export type SpinEvent =
  /** `PLAYSND( "InfOpen.wav" )`. */
  | { readonly kind: 'tick' }
  /** The computer's hand changed. */
  | { readonly kind: 'com'; readonly hand: number }
  /** `PlayMusic( BGM_IN_FITUP )`. */
  | { readonly kind: 'win' }
  /** The streak changed: refresh the caption. */
  | { readonly kind: 'wins'; readonly wins: number }
  /** FAIL: the DEFEAT box. */
  | { readonly kind: 'defeat' }
  /** FINISH: the end box, then the window closes. */
  | { readonly kind: 'finish' }
  /** The buttons are enabled again. */
  | { readonly kind: 'enable' };

export class KawiBawiBoSpin {
  status: 0 | 1 | 2 = 0;
  count = 0;
  delay = 1;
  /** The result of the server (`MP_*`), -1 while none. */
  result = -1;
  /** The own hand. */
  my = KAWI;
  /** The computer's hand shown. */
  com = KAWI;
  wins = 0;

  /** START: the hand starts to turn. */
  start(): void {
    this.status = 1;
  }

  /** `ReceiveResult`. */
  receive(state: number): void {
    this.result = state;
  }

  /** One 60 Hz frame. */
  step(out: SpinEvent[]): void {
    if (this.status === 1) {
      let target = -1;
      if (this.count > this.delay) {
        if (this.result > -1) {
          out.push({ kind: 'tick' });
          if (this.delay < 20) this.delay += 1;
          else target = comHand(this.my, this.result);
        }
        this.com = (this.com + 1) % 3;
        out.push({ kind: 'com', hand: this.com });
        if (this.com === target) {
          this.status = 2;
          this.delay = 40;
        }
        this.count = 0;
      }
      this.count += 1;
    } else if (this.status === 2) {
      if (this.count > this.delay) {
        if (this.result === MP.TRUE) {
          this.wins += 1;
          out.push({ kind: 'wins', wins: this.wins }, { kind: 'win' });
        } else if (this.result === MP.FINISH) {
          this.wins += 1;
          out.push({ kind: 'wins', wins: this.wins }, { kind: 'win' }, { kind: 'finish' });
        } else if (this.result === MP.FAIL) {
          this.wins = 0;
          out.push({ kind: 'defeat' }, { kind: 'wins', wins: 0 });
        }
        out.push({ kind: 'enable' });
        this.status = 0;
        this.delay = 1;
        this.result = -1;
      }
      this.count += 1;
    }
  }
}
