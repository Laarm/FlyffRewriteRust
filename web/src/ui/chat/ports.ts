// The ports of the chat window shells (Plan/42 §13.3-§13.4, implemented by 45 WP-CFEAT-06/07): what the window needs
// to read from, and what it asks the feature layer to do. `ui/` never imports `features/`.

import type { ChatLog } from './ChatLog';

/** What the chat window reads (45's `chatStore` + the stores behind the radio enabling). */
export interface ChatView {
  /** `g_Neuz.m_szWhisperName`: the last whisperer (`/r`), `null` before the first whisper. */
  lastWhisperer(): string | null;
  /** The own player is in a party (enables the party radio). */
  inParty(): boolean;
  /** The own player has a guild (enables the guild radio). */
  inGuild(): boolean;
  /** Authority >= GAMEMASTER: selection and copy from the text area. */
  isGm(): boolean;
  /** The own character name (right-click on a line starts a whisper unless it is the own line). */
  selfName(): string;
  /** `InstantHelp.inc` lines for the help tips (`options.instantHelp`), may be empty. */
  helpLines?(): readonly string[];
  /** The localised help prefix (`TID_SYS_HELP`). */
  helpPrefix?(): string;
  /**
   * Pointer events of the chat window that the world also wants (`CWndChat::OnLButtonDown` & co forward them: a click
   * through the chat window still moves the character); `pt` in root coordinates.
   */
  worldPointer?(
    kind: 'lbuttondown' | 'lbuttonup' | 'rbuttondown' | 'rbuttonup' | 'mousemove',
    pt: { x: number; y: number },
  ): void;
}

export interface ChatActions {
  /** The input pipeline of 45 (`ParsingChat`, §8.2): trim, cut, `/r`, commands, send. */
  submit(line: string): void;
  /** Gives the keyboard back to the world window after a line was sent (default: the UI root). */
  focusWorld?(): void;
}

/** The two commands lists (`APP_COMMAND`): the names the issuer may type, English long names. */
export interface CommandListView {
  /** The auto-complete names (authority filtered), sorted. */
  names(): readonly string[];
}

/** The chat-filter window's writes (45 persists them in `optionsStore.chatFilters`). */
export interface ChatFilterActions {
  setFilters(filters: readonly number[], etc: number): void;
}

export interface ChatPorts {
  view: ChatView;
  actions: ChatActions;
  commands: CommandListView;
  filter: ChatFilterActions;
}

/** The log the windows read. */
export interface ChatWindowDeps {
  readonly log: ChatLog;
}

/** Port of a world overlay (41, WP-REND-14): centre messages and balloons. */
export interface OverlaySink {
  clientMsg(text: string, color: number): void;
  balloon(objId: number, text: string, color: number, kind?: 0 | 1 | 2 | 3): void;
}

/** What an appended line looks like to a text window. */
export interface ChatTextTarget {
  /** `CWndChat::PutString`/`CWndChatLog::PutString`: appends one line (markup parsed). */
  put(text: string, color: number): void;
}

/** What the windows use before 45 installs real ports (tests, the gallery): nothing happens. */
export const NULL_CHAT_PORTS: ChatPorts = {
  view: {
    lastWhisperer: () => null,
    inParty: () => false,
    inGuild: () => false,
    isGm: () => false,
    selfName: () => '',
  },
  actions: { submit: () => undefined },
  commands: { names: () => [] },
  filter: { setFilters: () => undefined },
};
