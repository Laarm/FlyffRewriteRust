// The two message boxes of the rainbow race that live next to the main box (`CWndRainbowRaceMiniGameEnd` and `CWndRRMiniGameLadderFail`, `src:_Interface/WndRainbowRace.cpp:891-935, 2157-2186`): `MessageBoxWnd`
// subclasses on `APP_MESSAGEBOX` under keys of their own (`END_BOX_KEY`, `LADDER_FAIL_BOX_KEY`), so they never replace the main box (the legacy kept them in pointers of their own, and `msgbox.closeAll()` leaves
// them). Both run a callback when they go, by OK, Esc or the logout sweep (`onDestroy`).

import { MessageBoxWnd } from '../../msgbox/MessageBoxWnd';
import { MB } from '../../msgbox/types';

/** The text and what happens: `onOk` runs when OK is clicked (before the box goes), `onGone` whenever the box is destroyed. */
export interface RainbowBoxHooks {
  onOk?(): void;
  onGone?(): void;
}

class HookedBox extends MessageBoxWnd {
  readonly #hooks: RainbowBoxHooks;
  #gone = false;

  constructor(text: string, hooks: RainbowBoxHooks) {
    super(text, MB.OK);
    this.#hooks = hooks;
    this.onResult = () => {
      this.#hooks.onOk?.();
    };
  }

  override onDestroy(): void {
    if (!this.#gone) {
      this.#gone = true;
      this.#hooks.onGone?.();
    }
    super.onDestroy();
  }
}

/** `CWndRainbowRaceMiniGameEnd`: "Congratulations. You are the winner of <game>!". OK refreshes the progress list; the box going unfreezes it. */
export class RainbowGameEndBox extends HookedBox {}

/** `CWndRRMiniGameLadderFail`: "You have failed!"; the box going resets the ladder window. */
export class LadderFailBox extends HookedBox {}
