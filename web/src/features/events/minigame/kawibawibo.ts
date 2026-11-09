// The Kawibawibo machine (`CWndKawiBawiBoGame`, `src:_Interface/WndMiniGame.cpp:17-318`, `_Common/MiniGame.cpp:23-56, 634-643`, `Neuz/DPClient.cpp:11454-11510`; Plan/45 §28.0; WP-CFEAT-45): the rules
// behind the windows 619 / 620, in the sense of "windows hold no rules" (`ui/NOTES.md` WndSmeltSafety). The window calls `frame()` once per 60 Hz frame and reads `view()`; the replies of the server
// come in through `receive*()`; everything the machine causes outside itself (a cue, a box, a send, the win window) goes through `KawiOut`.
//
// Hands KAWI 0, BAWI 1, BO 2; results WIN 0, LOST 1, DRAW 2, FAILED 3, GETITEM 4.
//   * Start: no coupon (and `checkCoupon`) -> the box STARTGUID1 and the window closes, NOTHING is sent (FIX: the C++ went on and sent from a destroyed window); else the buttons are disabled, the
//     status turns 1 and `KawibawiboStart` goes out.
//   * Spin (status 1): the NPC hand advances every `delay + 1` frames; once the result is in, every step ticks and `delay` grows to 20, then the target is `you( my, result )` and the spin stops on
//     it (status 2, delay 40). After 40 frames: WIN -> the win cue, `winning += 1`, the win window; DRAW -> the buttons come back; LOST -> the box DEFEAT, `winning = 0`.
//   * GETITEM is NOT a round result (FIX: the C++ stored it in `m_nResult`, so the next spin decelerated at once); a WIN without the window is dropped (FIX: the C++ dereferenced NULL).

import { TID_GAME_KAWIBAWIBO_DEFEAT, TID_GAME_KAWIBAWIBO_STARTGUID1 } from '../../../gen/defs/text';
import { KAWI, BAWI, BO } from '../../../ui/windows/minigame/ports';
import type { KawibawiboRequest, KawibawiboView } from '../../../ui/windows/minigame/ports';

export { KAWI, BAWI, BO };

/** `CMiniGame::KAWIBAWIBO_*`. */
export const KAWI_RESULT = { WIN: 0, LOST: 1, DRAW: 2, FAILED: 3, GETITEM: 4 } as const;

/** `GetKawibawiboYou( nMy, nResult )`: the NPC's hand for the round the server decided; any other pair gives the own hand. */
export function kawibawiboYou(my: number, result: number): number {
  const table: readonly (readonly [number, number, number])[] = [
    [KAWI, KAWI_RESULT.LOST, BAWI],
    [KAWI, KAWI_RESULT.WIN, BO],
    [BAWI, KAWI_RESULT.WIN, KAWI],
    [BAWI, KAWI_RESULT.LOST, BO],
    [BO, KAWI_RESULT.LOST, KAWI],
    [BO, KAWI_RESULT.WIN, BAWI],
  ];
  for (const [m, r, you] of table) if (my === m && result === r) return you;
  return my;
}

/** The prizes of a win: the streak's item and the next one (`item_id`, `next_item_id`, the counts). */
export interface KawiPrizes {
  readonly itemId: number;
  readonly nextItemId: number;
  readonly count: number;
  readonly nextCount: number;
}

/** What the win window is told. */
export interface KawiWinInfo extends KawiPrizes {
  /** The Next button is disabled: the streak reached the highest. */
  readonly nextDisabled: boolean;
}

/** One reply (`MiniGame.KawibawiboResult`). */
export interface KawiReply {
  readonly result: number;
  readonly winCount: number;
  readonly itemId: number;
  readonly nextItemId: number;
  readonly itemCount: number;
  readonly nextItemCount: number;
  readonly maxWinCount: number;
}

/** What the machine causes outside itself. */
export interface KawiOut {
  /** `PLAYSND( "InfOpen.wav" )` (T39 tick). */
  tick(): void;
  /** `PlayMusic( BGM_IN_FITUP )` (T39 win). */
  win(): void;
  /** `g_WndMng.OpenMessageBox( prj.GetText( tid ) )`. */
  box(tid: number): void;
  /** `II_SYS_SYS_EVE_KAWIBAWIBO` (26430) is in the bag. */
  hasCoupon(): boolean;
  /** `SendKawibawiboStart`. */
  sendStart(): void;
  /** `SendKawibawiboGetItem`. */
  sendGetItem(): void;
  /** `CreateWinningWnd`: the win window (it re-enables the buttons when it closes: `winClosed`). */
  openWin(info: KawiWinInfo): void;
  /** `Destroy()` of the game window. */
  closeGame(): void;
}

/** The text ids the machine asks the boxes with. */
export const KAWI_BOX = {
  STARTGUID1: TID_GAME_KAWIBAWIBO_STARTGUID1,
  DEFEAT: TID_GAME_KAWIBAWIBO_DEFEAT,
} as const;

export class KawibawiboMachine implements KawibawiboRequest {
  /** `m_nMyChoice`, `m_nComChoice`. */
  myChoice: number = KAWI;
  comChoice: number = KAWI;
  /** `m_nWinningCount`, `m_nWinningMaxCount`. */
  winning = 0;
  maxWin = 0;
  /** `m_nCount`, `m_nDelay`, `m_nStatus`: 0 idle, 1 spinning, 2 showing the result. */
  count = 0;
  delay = 1;
  status = 0;
  /** `m_nResult` (-1 = none yet), `m_nPrevResult`. */
  result = -1;
  prevResult = -1;
  /** `m_bCheckCoupon`: the next Start needs a coupon (false after a draw or a "Next"). */
  checkCoupon = true;
  /** The buttons are enabled (`EnableAllBtn` / `DisableAllBtn`). */
  enabled = true;
  #prizes: KawiPrizes = { itemId: -1, nextItemId: -1, count: -1, nextCount: -1 };
  #closed = false;

  constructor(readonly out: KawiOut) {}

  view(): KawibawiboView {
    return {
      myChoice: this.myChoice,
      comChoice: this.comChoice,
      winning: this.winning,
      buttonsEnabled: this.enabled,
    };
  }

  /** `BTN_LEFT` / `BTN_RIGHT`: the own hand cycles. */
  cycle(dir: -1 | 1): void {
    this.myChoice = (this.myChoice + dir + 3) % 3;
  }

  /** `BTN_START`. */
  start(): void {
    if (this.checkCoupon && !this.out.hasCoupon()) {
      this.out.box(KAWI_BOX.STARTGUID1);
      this.out.closeGame();
      return;
    }
    this.enabled = false;
    this.status = 1;
    // the machine forgets the last reply: a late GETITEM must not slow the spin down
    this.result = -1;
    this.out.sendStart();
  }

  /** `ReceiveResult` for a round result (WIN, LOST, DRAW) and FAILED. */
  receive(r: KawiReply): void {
    if (r.result === KAWI_RESULT.FAILED) {
      this.out.box(KAWI_BOX.STARTGUID1);
      this.out.closeGame();
      return;
    }
    if (r.result === KAWI_RESULT.GETITEM) return;
    if (r.result === KAWI_RESULT.DRAW) this.checkCoupon = false;
    else if (r.result === KAWI_RESULT.LOST || r.result === KAWI_RESULT.WIN) this.checkCoupon = true;
    if (r.result === KAWI_RESULT.WIN) this.maxWin = r.maxWinCount;
    this.prevResult = this.result = r.result;
    this.#prizes = {
      itemId: r.itemId,
      nextItemId: r.nextItemId,
      count: r.itemCount,
      nextCount: r.nextItemCount,
    };
  }

  /** The GETITEM reply: the streak is paid out and starts again (`m_nWinningCount = 0`; the box is the reducer's). */
  gotItem(): void {
    this.winning = 0;
  }

  /** `Process()`: one 60 Hz frame. */
  frame(): void {
    if (this.status === 1) {
      if (this.count > this.delay) {
        let target = -1;
        if (this.result > -1) {
          this.out.tick();
          if (this.delay < 20) this.delay += 1;
          else target = kawibawiboYou(this.myChoice, this.result);
        }
        this.comChoice = (this.comChoice + 1) % 3;
        if (this.comChoice === target) {
          this.status = 2;
          this.delay = 40;
        }
        this.count = 0;
      }
      this.count += 1;
    } else if (this.status === 2) {
      if (this.count > this.delay) {
        if (this.result === KAWI_RESULT.WIN) {
          this.out.win();
          this.#createWin();
        } else if (this.result === KAWI_RESULT.DRAW) {
          this.enabled = true;
        } else if (this.result === KAWI_RESULT.LOST) {
          this.out.box(KAWI_BOX.DEFEAT);
          this.winning = 0;
          this.enabled = true;
        }
        this.status = 0;
        this.delay = 1;
        this.result = -1;
      }
      this.count += 1;
    }
  }

  /** `CreateWinningWnd`: the streak grows and the win window opens. */
  #createWin(): void {
    this.winning += 1;
    this.out.openWin({ ...this.#prizes, nextDisabled: this.winning >= this.maxWin });
  }

  /** `OnDestroy` of the win window: `RefreshInfo(); EnableAllBtn()`. */
  winClosed(): void {
    this.enabled = true;
  }

  /** `BTN_END` of the win window: the prize is asked for and the next Start needs a coupon again. */
  end(): void {
    this.out.sendGetItem();
    this.checkCoupon = true;
  }

  /** `BTN_NEXT`: go on at once, no coupon. */
  next(): void {
    this.checkCoupon = false;
  }

  /** `WTBID_CLOSE` of the game window. */
  canClose(): boolean {
    return !(this.winning > 0 || this.status !== 0 || this.prevResult === KAWI_RESULT.DRAW);
  }

  /** The window ended (the module drops its reference and closes the win window, the C++ destructor). */
  onClosed: (() => void) | null = null;
  closed(): void {
    this.#closed = true;
    this.onClosed?.();
  }
  get isClosed(): boolean {
    return this.#closed;
  }
}
