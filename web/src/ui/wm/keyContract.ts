// The window-side keyboard contract (Plan/42 §5.6; `CNeuzApp::MsgProc`, Neuz.cpp:1249-1534, ref/21 §8.1): which key
// presses reach the game, which are swallowed and which belong to the UI. Doc 43 owns the single `keydown` listener
// and the routing rules R1-R7; it asks these pure functions (plus `WindowManager.modalOpen()`, `isEditFocused()`, ...)
// for the window side, and the WP-UI-15 contract tests drive them with a fake router.

/** The window state a key decision depends on (all read from the `WindowManager`). */
export interface KeyState {
  /** A UI `<input>`/`<textarea>` has DOM focus (`CWndEdit::GetWndEditCtrl() != NULL`). */
  editFocused: boolean;
  /** The focused frame has `WBS_MODAL` (`IsOpenModalWnd`). */
  modalOpen: boolean;
  /** `APP_STATE_CONFIRM` is open (`KillWindow`). */
  stateConfirmOpen: boolean;
  /** The logout countdown box is open (`m_pLogOutWaitting`). */
  logoutCountdown: boolean;
  /** The chat window exists (`GetApplet(APP_COMMUNICATION_CHAT)`). */
  chatOpen: boolean;
}

/**
 * What happens to a `keydown`:
 *  * `edit`: the focused edit consumes it (typing); the game never sees it;
 *  * `killWindow`: `APP_STATE_CONFIRM` handles it (Enter = Yes, Esc = close) and nothing else sees it;
 *  * `enterToChat`: Enter while a modal is open without an edit focus: the chat window takes the focus
 *    (`focusChatOverModal`); the game does not see the key;
 *  * `focusChat`: Enter with the chat open and no edit focused: the chat edit takes the focus, the key
 *    continues to the game as Enter (43 decides what the world does with it);
 *  * `swallow`: nothing sees the key;
 *  * `game`: the key goes on to the game (hotkeys, the Esc chain, F-keys, the world).
 */
export type KeyVerdict = 'edit' | 'killWindow' | 'enterToChat' | 'focusChat' | 'swallow' | 'game';

/** `F1`..`F9` are the only function keys that escape a focused edit (`__CSC_VER9_3`); F10 is never a slot key (FIX, PL-124). */
export function isSlotFunctionKey(code: string): boolean {
  return /^F[1-9]$/.test(code);
}

/** The key passes a focused edit to the game: `Escape` and `F1`-`F9` (the typing rule of §5.6 item 1). */
export function escapesEdit(code: string): boolean {
  return code === 'Escape' || isSlotFunctionKey(code);
}

/**
 * Classifies a `keydown` for the window side (the order of `MsgProc`, modal or not):
 *  1. a focused edit keeps every key except Escape and F1-F9 (typing into an edit works during the logout
 *     countdown and with a modal open);
 *  2. the logout countdown swallows every other key;
 *  3. `APP_STATE_CONFIRM` takes the key (`KillWindow`: Enter = Yes, Esc = close, anything else is swallowed), even
 *     with another modal open;
 *  4. Enter with the chat open and no edit focused moves the focus to the chat edit (over a modal: the chat
 *     temporarily becomes modal, `enterToChat`);
 *  5. a modal frame blocks everything else (the Esc chain, hotkeys and F-keys are inactive, KEEP); without a modal
 *     the key is the game's.
 */
export function routeKeyDown(code: string, s: KeyState): KeyVerdict {
  const enter = code === 'Enter' || code === 'NumpadEnter';
  if (s.editFocused && !escapesEdit(code)) {
    // `KillWindow` still sees Enter while an edit has the focus (it does not eat Enter before the later steps)
    return enter && s.stateConfirmOpen && !s.logoutCountdown ? 'killWindow' : 'edit';
  }
  if (s.logoutCountdown) return 'swallow';
  if (s.stateConfirmOpen) return 'killWindow';
  if (enter && s.chatOpen && !s.editFocused) return s.modalOpen ? 'enterToChat' : 'focusChat';
  return s.modalOpen ? 'swallow' : 'game';
}

/** What a key does to `APP_STATE_CONFIRM` once `routeKeyDown` said `killWindow`: Enter = Yes, Esc = close. */
export function killWindowAction(code: string): 'yes' | 'close' | 'none' {
  if (code === 'Enter' || code === 'NumpadEnter') return 'yes';
  if (code === 'Escape') return 'close';
  return 'none';
}

/** The state of the game-side restrictions of §5.6 items 6 and 7. */
export interface HotkeyRestrictions {
  /** A shortcut is being dragged (`isDraggingShortcut()`). */
  draggingShortcut: boolean;
  /** `APP_VENDOREX_SELL` or `_BUY` is open (`isVendorWindowOpen()`). */
  vendorWindowOpen: boolean;
}

/**
 * Whether a key that already passed `routeKeyDown` as `game` may trigger its hotkey (§5.6 items 6, 7, 8):
 *  * while a shortcut is dragged: no applet hotkeys, no F-keys, no Enter;
 *  * while a vendor window is open: the page keys `Digit1`-`Digit8` and `Escape` are skipped (F-keys and applet
 *    letters still work);
 *  * `F10` is never a slot key and `Digit9`/`Digit0` are dropped (Plan/43 key table).
 */
export function hotkeyAllowed(code: string, r: HotkeyRestrictions): boolean {
  if (code === 'F10' || code === 'Digit9' || code === 'Digit0') return false;
  if (r.draggingShortcut) {
    if (isSlotFunctionKey(code) || code === 'Enter' || code === 'NumpadEnter') return false;
    if (/^Key[A-Z]$/.test(code)) return false; // applet letters
  }
  if (r.vendorWindowOpen) {
    if (/^Digit[1-8]$/.test(code) || code === 'Escape') return false;
  }
  return true;
}
