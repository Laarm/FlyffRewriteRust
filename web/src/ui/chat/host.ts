// What the chat windows need from their service (`ChatService`): the router, the ports of 45, the system log window and
// the few `g_WndMng` calls the original windows make. A narrow interface keeps the windows testable without a service.

import type { ChatLogWnd } from './ChatLogWnd';
import type { ChatPorts } from './ports';
import type { ChatRouter } from './ChatRouter';

export interface ChatHost {
  readonly router: ChatRouter;
  /** The ports of 45 (or the null ports before they are installed). */
  ports(): ChatPorts;
  /** `g_Option.m_nInstantHelp`. */
  instantHelp(): boolean;
  /** `g_WndMng.m_pWndChatLog`, `null` when it was never created or was destroyed. */
  logWindow(): ChatLogWnd | null;
  /** Creates the (hidden) system log window when it does not exist and returns it. */
  ensureLogWindow(): ChatLogWnd | null;
  /** `GetApplet(APP_WORLD)->SetFocus()` (the UI root when 45 gave no better target). */
  focusWorld(): void;
  /** The localised text of a TID (`''` when the bundle is missing). */
  text(tid: number): string;
}
