// The window ids, tab indices and timing constants of the mail windows, apart from the shells so that the service and the tests can read them without
// loading them (`ui/wnd/lazy.ts`).

/** The tab indices of the post window. */
export const TAB_SEND = 0;
export const TAB_RECEIVE = 1;
/** The window id of the list inside the inbox page (`m_wndScrollBar.Create(WBS_VERT, rect, this, 1000)` is its bar). */
export const WIDC_MAIL_LIST = 11;
/** The window id of the item slot control created over `WIDC_STATIC3` of the send tab, and of the receiver field. */
export const WIDC_SEND_ITEM = 0x7d01;
export const WIDC_SEND_RECEIVER = 0x7d02;
/** The window ids of the two slots created over `WIDC_STATIC3` (the item) and `WIDC_STATIC4` (the coin) of the reader. */
export const WIDC_READ_ITEM = 0x7e01;
export const WIDC_READ_COIN = 0x7e02;
/** `REQUEST_BUTTON_WAITING_TIME` (milliseconds) and `DESTRUCTION_TIME` (presses) of the requesting box. */
export const REQUEST_WAIT_MS = 5000;
export const DESTRUCTION_PRESSES = 3;
