// `ChatLog` (`CWndMgr::m_aChatString/Color/Style`, Plan/42 §13.1): the single ring of chat lines, session-scoped.
// Before adding a line, when the log holds MORE than 500 lines the oldest 250 are dropped (`WndManager.cpp:4213-4217`);
// newlines inside a line are replaced by spaces; `wm.openField()` clears it.

import { CHATSTY, ETC_BITS } from './styles';

/** One recorded line: the text with its markup, the colour and the (post-routing) chat style. */
export interface ChatLine {
  readonly text: string;
  /** `0xAARRGGBB`. */
  readonly color: number;
  /** `m_aChatStyle`: the style AFTER the SYSTEM/HELP/GAME rewrite of `putString` step 2. */
  readonly style: number;
  /** The speaker's object id when the line came from a mover. */
  readonly objId?: number;
}

/** The ring is trimmed when it holds more than this many lines ... */
export const CHAT_LOG_MAX = 500;
/** ... by this many oldest lines. */
export const CHAT_LOG_TRIM = 250;

export class ChatLog {
  #lines: ChatLine[] = [];
  #version = 0;

  /** Records a line (the trim rule first, then the line). */
  add(line: ChatLine): void {
    if (this.#lines.length > CHAT_LOG_MAX) this.#lines = this.#lines.slice(CHAT_LOG_TRIM);
    this.#lines.push({ ...line, text: line.text.replace(/\n/g, ' ') });
    this.#version += 1;
  }

  /** All lines, oldest first. */
  all(): readonly ChatLine[] {
    return this.#lines;
  }

  get size(): number {
    return this.#lines.length;
  }

  /** Increments with every change (cheap invalidation for the views). */
  get version(): number {
    return this.#version;
  }

  clear(): void {
    this.#lines = [];
    this.#version += 1;
  }

  /** The lines of a tab (`CWndChat::SetChannel`): style bits intersect `chatFilterEtc | chatFilter[tab]`. */
  forMask(mask: number): ChatLine[] {
    return this.#lines.filter((l) => (l.style & mask) !== 0);
  }
}

/** `true` for the style bits that belong to the etc part (help/system/game), tests. */
export const isEtcStyle = (style: number): boolean =>
  (style & ETC_BITS) !== 0 && style !== CHATSTY.GENERAL;
