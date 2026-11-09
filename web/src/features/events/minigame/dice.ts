// The dice machine (`CWndDiceGame`, `CWndDiceGameTender`, `CWndDiceGameMessage`, `src:_Interface/WndMiniGame.cpp:908-1596`, `Neuz/DPClient.cpp:11620-11650`; Plan/45 §28.0; WP-CFEAT-45): the "Five System"
// dice game behind the window 616. The player puts a stake on up to six faces (`FiveSystemBet`, the server keeps the stakes and answers `996 { penya, bet_num }`), presses Start and the die spins:
//   phase 0   a new face (never the current one) every 4 frames, one tick each, until the frame counter passes 170;
//   phase 1   the wait between two faces grows from 1 to 30 frames (a tick each);
//   phase 2   after 30 frames `FiveSystemStart` is sent ONCE (KEEP: the C++ asks for the roll after the spin), then the roll is awaited; when it is in, the result box opens:
//             a stake on the rolled face -> `TID_GAME_DICEGAME_SUCCESS( face + 1, payout )` and the win cue, else `TID_GAME_DICEGAME_FAIL`; the three rects of that face flash
//             (alpha 70 <-> 230, 15 per frame); OK resets the stakes and re-enables the buttons.
// FIX: the frame counter restarts at every Start (the C++ kept counting from the end of the last round, so every round after the first skipped its first phase).
// Replies 997 / 998 / 999 (refused bet, over the maximum, not enough penya) change nothing (KEEP: the C++ had empty branches).

import { TID_GAME_DICEGAME_ERROR } from '../../../gen/defs/text';
import type { DicePort, DiceView } from '../../../ui/windows/minigame/ports';

/** `FIVESYSTEM_BETOK` .. `_NOTENOUGH`. */
export const FIVE_RESULT = { BETOK: 996, FAILED: 997, OVERMAX: 998, NOTENOUGH: 999 } as const;

/** The defaults before `FIVESYSTEM_OPENWND` (`CMiniGame` constructor). */
export const DICE_DEFAULTS = { min: 1_000_000, max: 100_000_000, multiple: 5 } as const;

/** What the machine causes outside itself. */
export interface DiceOut {
  tick(): void;
  win(): void;
  /** `rand() % n`. */
  random(n: number): number;
  /** `GetNumberFormatEx`: thousands separators. */
  commas(n: number): string;
  box(tid: number): void;
  /** `TID_GAME_DICEGAME_SUCCESS` / `_FAIL` already filled in. */
  openMessage(text: string): void;
  /** The stake box for a face. */
  openTender(face: number, preset: number, min: number, max: number): void;
  /** `SendFiveSystemBet`. */
  sendBet(face: number, penya: number): void;
  /** `SendFiveSystemStart`. */
  sendStart(): void;
  /** `SendFiveSystemDestroyWnd`. */
  sendDestroy(): void;
  /** `~CWndDiceGame`: the stake box and the result box go with the window. */
  closeBoxes(): void;
  /** The texts with their ids filled in. */
  success(face: number, payout: string): string;
  fail(): string;
}

export class DiceMachine implements DicePort {
  min: number = DICE_DEFAULTS.min;
  max: number = DICE_DEFAULTS.max;
  multiple: number = DICE_DEFAULTS.multiple;
  readonly stakes: number[] = [0, 0, 0, 0, 0, 0];
  /** `m_nDiceNum`: the face drawn big. */
  face = 0;
  /** `m_nCount`, `m_nDelay`, `m_nStatus` (-1 idle, 0, 1, 2). */
  count = 0;
  delay = 1;
  status = -1;
  /** `m_nDiceChoiceNum`, `m_nPrizePenya`, `m_bReciveResult`, `m_bSendStart`, `m_bEnd`. */
  choice = -1;
  prize = 0;
  received = false;
  sentStart = false;
  end = false;
  /** `BTN_START` is enabled: only after `FIVESYSTEM_OPENWND` (and again after OK). */
  startEnabled = false;
  buttonsEnabled = true;
  /** `m_nAlpha`, `m_bFlsh`. */
  alpha = 0;
  rising = true;
  description = '';
  /** `FiveSystemStart` was sent (tests). */
  starts = 0;

  constructor(readonly out: DiceOut) {}

  view(): DiceView {
    return {
      face: this.face,
      stakes: this.stakes,
      multiple: this.multiple,
      startEnabled: this.startEnabled,
      buttonsEnabled: this.buttonsEnabled,
      flashFace: this.end && this.choice > -1 ? this.choice : null,
      flashAlpha: this.alpha,
      description: this.description,
    };
  }

  /** `FIVESYSTEM_OPENWND`: the limits of the server; Start becomes available. */
  setLimits(min: number, max: number, multiple: number): void {
    this.min = min;
    this.max = max;
    this.multiple = multiple;
    this.startEnabled = true;
  }

  /** `SetTenderPenya` (reply 996): the stake of a face; anything outside 0..5 is ignored. */
  setStake(face: number, penya: number): void {
    if (face >= 0 && face < 6) this.stakes[face] = penya;
  }

  /** `ReceiveResult( nDiceNum, nPenya )`: the roll. */
  receiveRoll(face: number, penya: number): void {
    if (face < 0 || face >= 6) return;
    this.face = face;
    this.choice = face;
    this.prize = penya;
    this.received = true;
  }

  /** `BTN_NUM1..6`: the stake box. */
  openStake(face: number): void {
    if (face < 0 || face >= 6) return;
    this.out.openTender(face, this.stakes[face] ?? 0, this.min, this.max);
  }

  /** The OK of the stake box: `CheckPenyaUsuable` = `SendFiveSystemBet`. */
  bet(face: number, penya: number): void {
    this.out.sendBet(face, penya);
  }

  /** `BTN_START`. */
  start(): void {
    if (!this.stakes.some((s) => s > 0)) {
      this.out.box(TID_GAME_DICEGAME_ERROR);
      return;
    }
    this.startEnabled = false;
    this.buttonsEnabled = false;
    this.status = 0;
    // FIX: every round spins its first phase (see the header)
    this.count = 0;
    this.delay = 1;
  }

  /** A face other than the current one. */
  #newFace(): void {
    let n = this.out.random(6);
    if (n === this.face) {
      n += 1;
      if (n > 5) n = 0;
    }
    this.face = n;
  }

  /** `Process()` and the flash of `OnDraw`: one 60 Hz frame. */
  frame(): void {
    if (this.status === 0) {
      if (this.count % 4 === 0) {
        if (this.count > 170) {
          this.count = 0;
          this.status = 1;
        } else {
          this.out.tick();
          this.#newFace();
        }
      }
      this.count += 1;
    } else if (this.status === 1) {
      if (this.count > this.delay) {
        if (this.delay < 30) {
          this.out.tick();
          this.delay += 1;
        } else {
          this.out.tick();
          this.status = 2;
          this.delay = 30;
        }
        this.#newFace();
        this.count = 0;
      }
      this.count += 1;
    } else if (this.status === 2) {
      if (this.count > this.delay) {
        if (this.received) {
          const stake = this.stakes[this.choice] ?? 0;
          if (stake > 0) {
            this.out.openMessage(this.out.success(this.choice + 1, this.out.commas(this.prize)));
            this.out.win();
          } else {
            this.out.openMessage(this.out.fail());
          }
          this.status = -1;
          this.delay = 1;
          this.received = false;
          this.end = true;
        } else if (!this.sentStart) {
          this.out.sendStart();
          this.starts += 1;
          this.sentStart = true;
        }
      }
      this.count += 1;
    }
    // the flash of the rolled face (drawn per frame in the original's `OnDraw`)
    if (this.end && this.choice > -1) {
      if (this.rising) {
        this.alpha += 15;
        if (this.alpha > 230) {
          this.alpha = 230;
          this.rising = false;
        }
      } else {
        this.alpha -= 15;
        if (this.alpha < 70) {
          this.alpha = 70;
          this.rising = true;
        }
      }
    }
  }

  /** OK of the result box: the stakes are reset and the controls come back (`ResetPenya`, `RefreshCtrl`, `RefreshInfo`). */
  messageOk(): void {
    this.stakes.fill(0);
    this.startEnabled = true;
    this.buttonsEnabled = true;
    this.end = false;
    this.sentStart = false;
  }

  /** The window ended (the module drops its reference). */
  onClosed: (() => void) | null = null;

  /** The window ended (`OnDestroy`): the server refunds the stakes. */
  closed(): void {
    this.out.sendDestroy();
    this.out.closeBoxes();
    this.onClosed?.();
  }
}
