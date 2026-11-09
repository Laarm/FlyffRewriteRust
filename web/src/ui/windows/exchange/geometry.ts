// The numbers of the exchange window that the service and the tests share with the shell (kept out of the shell so that only `lazy.ts` imports it).

/** `MAX_LIST_COUNT`: up to this many entries the window shrinks to the list. */
export const MAX_LIST_COUNT = 15;
/** The extra width the English texts need (`rect.right += 80`). */
export const WIDEN = 80;
/** The buttons move right by half of it. */
export const BUTTON_SHIFT = 40;
/**
 * The server answers nothing to a request it ignores (the NPC is out of range, a trade is open: VR-012, VR-024), and the original's OK then stays disabled for
 * good; here it works again after this long without an answer.
 */
export const ANSWER_TIMEOUT_MS = 10_000;
