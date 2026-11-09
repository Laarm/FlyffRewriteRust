// The roll of the dice game (`CWndRRMiniGameDice::Process`, `src:_Interface/WndRainbowRace.cpp:1367-1525`, Plan/42 §19.4 P; WP-UI-61) as a pure state machine over 60 Hz frames, with the counters of the original.
// Die 1: phase 0 changes face on every 4th frame (72 frames), phase 1 settles with growing delays 1..22 and lands on the result; die 2 starts on the first 4th frame past 30 (frame 32), rolls until the
// first 4th frame past its 100, settles with delays 1..24, lands and waits 40 frames; then FINISH closes the window with the end box, anything else enables START again with the backup target.
// FIX: die 1 leaves phase 0 and die 2 lands only once the result is in (the original assumed the answer within ~1.2 s and could land on a stale face).

import { MP } from '../constants';
import type { Rng } from '../rng';

/** What a frame asks the window to do. */
export type DiceEvent =
  /** `PLAYSND( "InfOpen.wav" )`. */
  | { readonly kind: 'tick' }
  /** A die changed face (die 1 or 2, face 0..5). */
  | { readonly kind: 'face'; readonly die: 1 | 2; readonly face: number }
  /** `PlayMusic( BGM_IN_FITUP )`. */
  | { readonly kind: 'win' }
  /** FINISH: the end box, then the window closes. */
  | { readonly kind: 'finish' }
  /** START is enabled again and the target number shows the backup. */
  | { readonly kind: 'reset'; readonly target: number };

export class DiceRoll {
  face1 = 0;
  face2 = 5;
  count1 = 0;
  count2 = 0;
  delay1 = 1;
  delay2 = 1;
  status1 = -1;
  status2 = -1;
  finished1 = false;
  /** The target number shown. */
  target = 0;
  backup = 0;
  /** The result of the server (`MP_*`) of the current roll, -1 while none. */
  result = -1;
  #res1 = 0;
  #res2 = 0;
  readonly #rng: Rng;

  constructor(rng: Rng) {
    this.#rng = rng;
  }

  /** A roll is on its way (the START button). */
  get rolling(): boolean {
    return this.status1 !== -1 || this.status2 !== -1;
  }

  /** START. */
  start(): void {
    this.status1 = 0;
    this.finished1 = false;
    this.result = -1;
  }

  /** `ReceiveResult`: a `FAIL` with `p2 = -1` only stores the backup target. */
  receive(state: number, p1: number, p2: number): void {
    if (state === MP.FAIL && p2 === -1) {
      this.backup = p1;
      return;
    }
    this.result = state;
    this.#res1 = p1 - 1;
    this.#res2 = p2 - 1;
  }

  /** A random face different from the current one (`rand() % 6`, +1 on a repeat, wrapping). */
  #other(current: number): number {
    let n = this.#rng(6);
    if (n === current) {
      n += 1;
      if (n > 5) n = 0;
    }
    return n;
  }

  /** One 60 Hz frame. */
  step(out: DiceEvent[]): void {
    const hasResult = this.result !== -1;
    if (this.status1 === 0) {
      if (this.count1 % 4 === 0) {
        if (this.status2 === -1 && this.count1 > 30) this.status2 = 0;
        if (this.count1 > 70 && hasResult) {
          this.count1 = 0;
          this.status1 = 1;
        } else {
          out.push({ kind: 'tick' });
          this.face1 = this.#other(this.face1);
          out.push({ kind: 'face', die: 1, face: this.face1 });
        }
      }
      this.count1 += 1;
    } else if (this.status1 === 1) {
      if (this.count1 > this.delay1) {
        out.push({ kind: 'tick' });
        if (this.delay1 < 22) {
          this.delay1 += 1;
          this.face1 = this.#other(this.face1);
          out.push({ kind: 'face', die: 1, face: this.face1 });
          this.count1 = 0;
        } else {
          this.face1 = this.#res1;
          out.push({ kind: 'face', die: 1, face: this.face1 });
          this.finished1 = true;
          this.status1 = -1;
        }
      }
      this.count1 += 1;
    }

    if (this.status2 === 0) {
      if (this.count2 % 4 === 0) {
        if (this.count2 > 100) {
          this.count2 = 0;
          this.status2 = 1;
        } else {
          if (this.finished1) out.push({ kind: 'tick' });
          this.face2 = this.#other(this.face2);
          out.push({ kind: 'face', die: 2, face: this.face2 });
        }
      }
      this.count2 += 1;
    } else if (this.status2 === 1) {
      if (this.count2 > this.delay2) {
        if (this.finished1) out.push({ kind: 'tick' });
        if (this.delay2 < 24) {
          this.delay2 += 1;
          this.face2 = this.#other(this.face2);
          out.push({ kind: 'face', die: 2, face: this.face2 });
          this.count2 = 0;
        } else if (hasResult) {
          this.face2 = this.#res2;
          out.push({ kind: 'face', die: 2, face: this.face2 });
          this.status2 = 2;
          this.delay2 = 40;
          this.count2 = 0;
        } else {
          this.count2 = 0;
        }
      }
      this.count2 += 1;
    } else if (this.status2 === 2) {
      if (this.count2 > this.delay2) {
        if (this.finished1 && this.result === MP.FINISH) {
          out.push({ kind: 'win' }, { kind: 'finish' });
        } else {
          this.count1 = 0;
          this.count2 = 0;
          this.delay1 = 1;
          this.delay2 = 1;
          this.status1 = -1;
          this.status2 = -1;
          this.finished1 = false;
          this.target = this.backup;
          out.push({ kind: 'reset', target: this.target });
        }
      }
      this.count2 += 1;
    }
  }
}
