// Ports of the duel and resurrection window shells (Plan/42 §19.4 K / C, WP-UI-41; wired by 45 WP-CFEAT-36). Each window is
// opened with ONE request object: the text it shows and the callbacks of its buttons. The decisions behind the callbacks (the
// clean-up of the transactional windows before `DuelYes`, the motions of the result, the sends) are 45's; the shells keep the
// control behaviour of the original (`src:_Interface/WndDuelConfirm.cpp`, `WndField.cpp:13307-13370`).

/**
 * `CWndDuelConfirm` (`APP_DUEL_CONFIRM`): "`<name>` challenges you to a duel. Do you accept?" (`TID_DIAG_0066`) or the party
 * version (`TID_DIAG_0067`). The window is modal and Enter is Yes (Plan/42 WP-UI-41). It never answers by itself: a window that
 * is closed by the program (own death, a newer request, the duel started) sends nothing.
 */
export interface DuelConfirmRequest {
  /** The question with the challenger's name filled in. */
  readonly text: string;
  /** Yes / Enter: the caller closes the transactional windows (solo duel only) and sends `DuelYes` / `DuelPartyYes`. */
  yes(): void;
  /** No / the close button of the title bar: `DuelNoReq` / `DuelPartyNoReq`. */
  no(): void;
  /** The window went away by any route (after `yes` / `no` too): the caller forgets the request. */
  closed?(): void;
}

/** `CWndDuelResult` (`APP_DUEL_RESULT`): "`<opponent>` loses / wins the Duel!" with one OK button (not modal). */
export interface DuelResultRequest {
  /** The line with the opponent's name filled in (`TID_DIAG_DUELRESULT_WIN` / `_LOSE`). */
  readonly text: string;
  /** The own character won. */
  readonly win: boolean;
  /** OK or the close button: the winner plays `MTI_HIPKIDDING` once, the loser `MTI_HEAD` looped, then the move state is sent. */
  done(): void;
  closed?(): void;
}

/** `CWndResurrectionConfirm` (`APP_RESURRECTION`): another player offers to resurrect the dead player; modal, no title bar. */
export interface ResurrectionRequest {
  /** Yes / Enter: `ResurrectionOk`. */
  yes(): void;
  /** No: `ResurrectionCancel`. */
  no(): void;
  closed?(): void;
}
