// `ChatRouter.putString` (port of `CWndMgr::PutString`, WndManager.cpp:4116-4264; Plan/42 §13.2): the generic targets
// of every chat line. 45 decides the text, colour, style and speaker of each message and calls
// `ui.chat.putString(text, objId, color, chatStyle, speaker?)`; this router decides where the line goes:
//
//  1. the chat tabs whose mask matches the style (`CHAT`; such a line is not repeated in the system log);
//  2. the etc part: SYSTEM / HELP / GAME styles go to the centre client messages (`CLIENT`) or the chat box (`CHAT`) and
//     the stored style becomes `*_CHAT`;
//  3. speaker rules: a mover speaker with a general-chat style gets a balloon (`DIALOG`); a non-player mover is not
//     written to the chat box and not recorded; an open shop, an invisible GM and a quiz-skipped player get no balloon;
//  4. `CLIENT` -> `overlays.clientMsg`; the `ChatLog` records the line; `CHAT` -> the chat window; `DIALOG` ->
//     `overlays.balloon(objId, text after the first ": ", ...)`; the system log window gets what is neither a balloon
//     nor a shout and not already in the chat box.

import { ChatLog } from './ChatLog';
import type { ChatTextTarget, OverlaySink } from './ports';
import { CHANNELS, CHATSTY, effectiveChatFilters } from './styles';

/** Who spoke, from 45's display rules (§8.3). */
export interface ChatSpeaker {
  /** The speaker is a player mover (non-player movers: balloon only, the line is not recorded). */
  isPlayer: boolean;
  /** The speaker has an open personal shop: no balloon. */
  vendorOpen: boolean;
  /** `authority >= GAMEMASTER` with `TRANSPARENT_MODE`: no balloon. */
  invisibleGm: boolean;
  /** The quiz world and the speaker has `QUIZ_RENDER_SKIP_MODE`: no balloon. */
  quizSkip: boolean;
  /** 0 normal, 1 party, 2 guild, 3 shout (§8.5). A non-zero kind asks for a balloon even without the general style. */
  balloonKind?: 0 | 1 | 2 | 3;
  /** The balloon text when it is not the text after `": "` of the line (shouts, party and guild lines). */
  balloonText?: string;
}

export interface ChatRouterDeps {
  /** The effective channel filters and the etc mask (45's options; the legacy defaults when unset). */
  options: () => { chatFilters: readonly number[]; chatFilterEtc: number };
  /** The overlay sink of 41; `null` before the renderer exists. */
  overlays: () => OverlaySink | null;
  /** The chat window's text target, `null` when the window is not open (`GetWndBase(APP_COMMUNICATION_CHAT)`). */
  chatWindow: () => ChatTextTarget | null;
  /** The system log window's text target, `null` when it does not exist. */
  logWindow: () => ChatTextTarget | null;
}

/** What `putString` decided for a line (tests, the probe). */
export interface RoutedLine {
  readonly chat: boolean;
  readonly client: boolean;
  readonly dialog: boolean;
  readonly recorded: boolean;
  readonly toSystemLog: boolean;
  /** The style that was stored (`dwChatStyle` after the etc rewrite). */
  readonly style: number;
}

export class ChatRouter {
  readonly log = new ChatLog();
  /** `CWndChat::m_nChatChannel` as a tab index 0..4. */
  currentTab = 0;
  readonly #d: ChatRouterDeps;

  constructor(deps: ChatRouterDeps) {
    this.#d = deps;
  }

  /** The channel masks in force (stored options or the legacy defaults) and the etc mask. */
  masks(): { filters: readonly number[]; etc: number } {
    const o = this.#d.options();
    return effectiveChatFilters(o.chatFilters, o.chatFilterEtc);
  }

  /** The mask of a tab for the refill (`etc | filter[tab]`). */
  tabMask(tab: number): number {
    const m = this.masks();
    return (m.etc | (m.filters[tab] ?? 0)) >>> 0;
  }

  /** `lines(tab)`: the recorded lines that belong to a tab. */
  lines(tab: number) {
    return this.log.forMask(this.tabMask(tab));
  }

  clear(): void {
    this.log.clear();
  }

  putString(
    text: string,
    objId: number | null,
    color: number,
    chatStyle: number,
    speaker?: Partial<ChatSpeaker>,
  ): RoutedLine {
    const { filters, etc } = this.masks();
    let style = chatStyle;
    let chat = false;
    let client = false;
    let dialog = false;
    let toLog = true;
    // 1. the tab filter of the current channel
    const filter = filters[this.currentTab] ?? 0;
    for (const c of CHANNELS) {
      if ((filter & c) === chatStyle) {
        chat = true;
        toLog = false;
      }
    }
    // 2. the etc rewrite
    const pairs: [number, number, number, number][] = [
      [CHATSTY.SYSTEM_CLIENT, CHATSTY.SYSTEM_CHAT, CHATSTY.SYSTEM, CHATSTY.SYSTEM_CHAT],
      [CHATSTY.HELP_CLIENT, CHATSTY.HELP_CHAT, CHATSTY.HELP, CHATSTY.HELP_CHAT],
      [CHATSTY.GAME_CLIENT, CHATSTY.GAME_CHAT, CHATSTY.GAME, CHATSTY.GAME_CHAT],
    ];
    for (const [clientBit, chatBit, group, chatStyleOut] of pairs) {
      if (chatStyle === group) {
        if ((etc & clientBit) !== 0) {
          client = true;
          style = chatStyleOut;
        }
        if ((etc & chatBit) !== 0) {
          chat = true;
          style = chatStyleOut;
        }
      }
    }
    // 3. the speaker
    let recorded = true;
    const kind = speaker?.balloonKind ?? 0;
    if (objId !== null && speaker !== undefined) {
      if ((chatStyle & CHATSTY.GENERAL) !== 0 || kind !== 0) dialog = true;
      if (speaker.isPlayer === false) {
        chat = false;
        recorded = false;
      }
      if (speaker.vendorOpen === true) dialog = false;
      if (speaker.invisibleGm === true) dialog = false;
      if (speaker.quizSkip === true) dialog = false;
    }
    // 4. the targets
    const overlays = this.#d.overlays();
    if (client) overlays?.clientMsg(text, color);
    const line = text.replace(/\n/g, ' ');
    if (recorded) {
      this.log.add({ text: line, color, style, ...(objId !== null ? { objId } : {}) });
    }
    if (chat) this.#d.chatWindow()?.put(line, color);
    let toSystemLog = false;
    if (dialog && objId !== null) {
      const balloon = speaker?.balloonText ?? textAfterColon(text);
      overlays?.balloon(objId, balloon, 0xffffffff, kind);
    }
    if (toLog && !dialog && style !== CHATSTY.SHOUT) {
      const w = this.#d.logWindow();
      if (w) {
        w.put(text, color);
        toSystemLog = true;
      }
    }
    return { chat, client, dialog, recorded, toSystemLog, style };
  }
}

/** The balloon text of a `"Name : text"` line: after the first `:` plus the two characters `": "`. */
export function textAfterColon(line: string): string {
  const i = line.indexOf(':');
  return i < 0 ? '' : line.slice(i + 2);
}

/** The character name of a chat line for the right-click whisper (`CWndChat::OnRButtonDown`), or `null`. */
export function whisperTargetOfLine(
  line: string,
  selfName: string,
  announceTag = 'Notice',
): string | null {
  // normal chat: "Name : text"
  const nameEnd = line.indexOf(' :');
  if (nameEnd > 0 && nameEnd < 32) {
    const firstSpace = line.indexOf(' ');
    if (firstSpace === -1 || nameEnd <= firstSpace) {
      const name = line.slice(0, nameEnd);
      if (name !== selfName) return name;
    }
  }
  // whisper: "<- [name] text" or "-> [name] text"
  const m = /^(?:<-|->) \[([^\]\s][^\]]*)\]/.exec(line);
  const whisperer = m?.[1];
  if (whisperer !== undefined && whisperer !== selfName) return whisperer;
  // shout: "[name] text" (an announcement "[Notice]" is excluded)
  const s = /^\[([^\]\s][^\]]*)\]/.exec(line);
  const shouter = s?.[1];
  if (shouter !== undefined && shouter !== selfName && shouter !== announceTag) return shouter;
  return null;
}
