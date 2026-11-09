// `ChatService` (`ui.chat`, Plan/42 §13): the chat router, the ring, the window shells and the ports of 45.
//
//   ui.chat.putString(text, objId, color, chatStyle, speaker?)   45's display rules (§8.3) write every line through it
//   ui.chat.setPorts({view, actions, commands, filter})            WP-CFEAT-06/07 install their implementations
//   ui.chat.setOverlays(sink)                                      41 (WP-REND-14): centre messages and balloons
//   ui.chat.focusInput()                                           Enter (43 §3.7): opens/focuses the chat edit
//
// The service registers the factories of `APP_COMMUNICATION_CHAT` and `APP_CHAT_LOG` with the applet registry; the
// filter window and the command list are children of the chat window.

import { APP_CHAT_LOG, APP_COMMUNICATION_CHAT } from '../../gen/defs/resdata';
import type { Rect } from '../layout/geometry';
import type { UiOptionsStore } from '../options';
import type { WindowManager } from '../wm/WindowManager';
import { ChatLogWnd } from './ChatLogWnd';
import { ChatRouter, type ChatSpeaker, type RoutedLine } from './ChatRouter';
import { ChatWnd } from './ChatWnd';
import { APP_COMMAND_HELP, CommandHelpWnd, type CommandHelpView } from './CommandHelpWnd';
import type { ChatHost } from './host';
import { type ChatPorts, NULL_CHAT_PORTS, type OverlaySink } from './ports';

export { appendChatLine, atBottom, TEXT_TRIM_AT, TEXT_TRIM_DROP } from './appendLine';
export { ChatFilterWnd } from './ChatFilterWnd';
export { ChatLog, CHAT_LOG_MAX, CHAT_LOG_TRIM } from './ChatLog';
export type { ChatLine } from './ChatLog';
export { ChatLogWnd } from './ChatLogWnd';
export { ChatRouter, textAfterColon, whisperTargetOfLine } from './ChatRouter';
export type { ChatSpeaker, RoutedLine } from './ChatRouter';
export {
  ChatEdit,
  ChatText,
  ChatWnd,
  FADE_AFTER_MS,
  HELP_INTERVAL_MS,
  MAX_HISTORY_STRING,
} from './ChatWnd';
export { CommandWnd, commandListHeight } from './CommandWnd';
export {
  APP_COMMAND_HELP,
  COMMAND_HELP_LIST_ID,
  CommandHelpWnd,
  type CommandHelpView,
} from './CommandHelpWnd';
export { chatGeometry, CHAT_MIN_H, CHAT_MIN_W, logWindowX } from './geometry';
export { MacroChatCtrl, cutBytes } from './MacroChatCtrl';
export { NULL_CHAT_PORTS } from './ports';
export type {
  ChatActions,
  ChatFilterActions,
  ChatPorts,
  ChatTextTarget,
  ChatView,
  CommandListView,
  OverlaySink,
} from './ports';
export {
  CHANNELS,
  CHATSTY,
  DEFAULT_CHAT_FILTER_ETC,
  DEFAULT_CHAT_FILTERS,
  effectiveChatFilters,
  ETC_BITS,
  tabPrefill,
} from './styles';

export interface ChatServiceDeps {
  readonly wm: WindowManager;
  readonly options: UiOptionsStore;
  /** `getText(tid)` of the texts bundle (`''` while it is missing). */
  readonly getText: (tid: number) => string;
}

export class ChatService implements ChatHost {
  readonly router: ChatRouter;
  readonly #wm: WindowManager;
  readonly #options: UiOptionsStore;
  readonly #getText: (tid: number) => string;
  #ports: ChatPorts;
  #overlays: OverlaySink | null = null;

  constructor(deps: ChatServiceDeps) {
    this.#wm = deps.wm;
    this.#options = deps.options;
    this.#getText = deps.getText;
    this.#ports = {
      ...NULL_CHAT_PORTS,
      // until 45 persists the filters the options store keeps them as an override on top of the host's values
      filter: {
        setFilters: (filters, etc) => {
          this.#options.set({ chatFilters: [...filters], chatFilterEtc: etc });
        },
      },
    };
    this.router = new ChatRouter({
      options: () => {
        const o = this.#options.get();
        return { chatFilters: o.chatFilters, chatFilterEtc: o.chatFilterEtc };
      },
      overlays: () => this.#overlays,
      chatWindow: () => this.chatWindow(),
      logWindow: () => this.logWindow(),
    });
    this.#wm.applets.register({
      app: APP_COMMUNICATION_CHAT,
      factory: () => new ChatWnd(this),
    });
    this.#wm.applets.register({ app: APP_CHAT_LOG, factory: () => new ChatLogWnd(this) });
    this.#wm.appletHooks = {
      ...this.#wm.appletHooks,
      clearChat: () => {
        this.clear();
      },
    };
  }

  // ---- ports -------------------------------------------------------------------------------------------------

  /** Installs the ports of 45 (each group replaces the default of that group). */
  setPorts(ports: Partial<ChatPorts>): void {
    this.#ports = { ...this.#ports, ...ports };
    this.chatWindow()?.refreshControlState();
  }

  ports(): ChatPorts {
    return this.#ports;
  }

  /** 41's sink for centre client messages and balloons (`null` removes it). */
  setOverlays(sink: OverlaySink | null): void {
    this.#overlays = sink;
  }

  // ---- the lines ---------------------------------------------------------------------------------------------

  /** `CWndMgr::PutString`: see {@link ChatRouter.putString}. */
  putString(
    text: string,
    objId: number | null,
    color: number,
    chatStyle: number,
    speaker?: Partial<ChatSpeaker>,
  ): RoutedLine {
    return this.router.putString(text, objId, color, chatStyle, speaker);
  }

  lines(tab: number) {
    return this.router.lines(tab);
  }

  /** `openField`: the ring is emptied (the windows keep what they show until they are rebuilt). */
  clear(): void {
    this.router.clear();
    this.chatWindow()?.setChannel();
    this.logWindow()?.text.setString('');
  }

  // ---- windows -----------------------------------------------------------------------------------------------

  chatWindow(): ChatWnd | null {
    const w = this.#wm.get(APP_COMMUNICATION_CHAT);
    return w instanceof ChatWnd ? w : null;
  }

  logWindow(): ChatLogWnd | null {
    const w = this.#wm.get(APP_CHAT_LOG);
    return w instanceof ChatLogWnd ? w : null;
  }

  ensureLogWindow(): ChatLogWnd | null {
    const existing = this.logWindow();
    if (existing) return existing;
    const w = this.#wm.open(APP_CHAT_LOG, () => new ChatLogWnd(this));
    if (!(w instanceof ChatLogWnd)) return null;
    w.setVisible(false);
    return w;
  }

  /**
   * The window of the browser's `/help` (BFIX1 #006): opens the list, or refreshes the one that is open (there is never more than one).
   * `null` when the window could not be created.
   */
  openCommandHelp(view: CommandHelpView): CommandHelpWnd | null {
    const open = this.#wm.get(APP_COMMAND_HELP);
    const w = this.#wm.open(APP_COMMAND_HELP, () => new CommandHelpWnd(view));
    if (!(w instanceof CommandHelpWnd)) return null;
    // an open window was only focused by `open`: it shows the new answer
    if (w === open) w.setRows(view);
    return w;
  }

  /** The state of the `/help` window for the e2e probe; `null` while it is closed. */
  commandHelp(): { title: string; rows: string[]; scroll: number; rect: Rect } | null {
    const w = this.#wm.get(APP_COMMAND_HELP);
    if (!(w instanceof CommandHelpWnd)) return null;
    const r = w.rectWindow;
    return {
      title: w.getTitle(),
      rows: w.list.state.get().items.map((i) => i.text),
      scroll: w.list.state.get().scroll,
      rect: { l: r.l, t: r.t, r: r.r, b: r.b },
    };
  }

  /**
   * Enter (43 §3.7): the chat window is opened when it is closed and its edit gets the keyboard; with a modal window
   * open the chat window temporarily becomes modal itself (`focusChatOverModal`, ended again when the line was sent).
   * Returns `false` when there is no chat window.
   */
  focusInput(): boolean {
    const wm = this.#wm;
    if (wm.modalOpen()) {
      if (!wm.focusChatOverModal()) return false;
      return this.chatWindow()?.focusInput() ?? false;
    }
    if (!this.chatWindow()) wm.objectExecutorApplet(APP_COMMUNICATION_CHAT);
    return this.chatWindow()?.focusInput() ?? false;
  }

  // ---- ChatHost ----------------------------------------------------------------------------------------------

  instantHelp(): boolean {
    return this.#options.get().instantHelp;
  }

  focusWorld(): void {
    const actions = this.#ports.actions;
    if (actions.focusWorld) actions.focusWorld();
    else this.#wm.root.setFocus();
  }

  text(tid: number): string {
    return this.#getText(tid);
  }
}
