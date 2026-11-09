// `WindowManagerApi` and `TaskbarApi` (Plan/42 §5.6): the `wm.*` and `taskbar.*` operations that the input layer (43:
// key routing, the Esc chain) and the feature modules (45) call. WP-UI-04 implements the window-state part
// (`WindowManagerCore`, implemented by `WindowManager`); the applet registry operations (`openApplet`,
// `objectExecutorApplet`, `openStartMenu`, `openField`) arrive with WP-UI-14/15, which complete the full interface.

import type { WndNeuz } from '../wnd/WndNeuz';

export interface WindowManagerApi {
  /** `CreateApplet` semantics (§5.9): an existing instance (same app and key) is focused and returned. */
  open<T extends WndNeuz>(app: number, build?: () => T, key?: string): T | null;
  get(app: number, key?: string): WndNeuz | null;
  isOpen(app: number): boolean;
  close(app: number, key?: string): void;
  /** Open or focus (`CreateApplet`), never toggles. */
  openApplet(app: number): void;
  /** The §12.2 toggle policy. */
  objectExecutorApplet(app: number): void;
  /** The §12.4 world-entry sequence. */
  openField(): void;
  /** `GetWndBase()` rule of the Esc chain (§5.7). */
  topClosableWindow(): WndNeuz | null;
  destroy(w: WndNeuz): void;
  /** §11.4: the start menu above the taskbar. */
  openStartMenu(aboveTaskbar: true): void;
  modalOpen(): boolean;
  focusedApp(): number | null;
  anyOpen(apps: readonly number[]): boolean;
  /** Enter while a modal is open (§5.2). */
  focusChatOverModal(): boolean;
  isDraggingShortcut(): boolean;
  cancelDragUnlessFrom(app: number): void;
  /** `APP_VENDOREX_SELL` or `APP_VENDOREX_BUY` is open. */
  isVendorWindowOpen(): boolean;
  /** Numpad `*` (§5.10). */
  clearAllWindows(): void;
  restoreWindows(): void;
  /** A UI `<input>`/`<textarea>` has DOM focus. */
  isEditFocused(): boolean;
  /** The logout countdown window exists (§5.6 rule 3: the game gets no keys). */
  isLogoutPending(): boolean;
  /**
   * Closes every in-game window when the session leaves the world (`DestroyApplet` + `Free`, BFIX1 #021): the frames
   * inserted while the field scope was on; returns how many. Idempotent.
   */
  closeField(): number;
}

/** The part of {@link WindowManagerApi} that the window manager core implements (WP-UI-04). */
export type WindowManagerCore = Pick<
  WindowManagerApi,
  | 'open'
  | 'get'
  | 'isOpen'
  | 'close'
  | 'topClosableWindow'
  | 'destroy'
  | 'modalOpen'
  | 'focusedApp'
  | 'anyOpen'
  | 'focusChatOverModal'
  | 'isDraggingShortcut'
  | 'cancelDragUnlessFrom'
  | 'isVendorWindowOpen'
  | 'clearAllWindows'
  | 'restoreWindows'
  | 'isEditFocused'
  | 'isLogoutPending'
>;

/** The taskbar operations 43 calls (§11.3; WP-UI-14). */
export interface TaskbarApi {
  executeItemSlot(i: number): void;
  setItemPage(n: number): void;
  page(): number;
  slotIsEquipItem(i: number): boolean;
}
