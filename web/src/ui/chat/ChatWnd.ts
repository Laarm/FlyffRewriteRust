// `ChatWnd` (`CWndChat`, WndCommunication.cpp:149-1310; Plan/42 §13.3): the chat window shell on
// `APP_COMMUNICATION_CHAT` (528 x 128, `WndTile308`, MOVE | THICKFRAME, no caption).
//
//  * Controls: the five channel radios, the filter button, the chat lock, the move lock, the system log toggle, the
//    help/close buttons from the layout; the macro icon, the input edit and the text area are created by code over the
//    layout placeholders (`WIDC_MACRO_CHAT`, `WIDC_EDIT`, `WIDC_CUSTOM1`).
//  * Text: lines arrive from `ChatRouter` through `put` (markup parsed, newline appended, sticky bottom scroll, 1000
//    lines -> drop the first 500); `setChannel` refills the area from the `ChatLog` for the current tab without parsing
//    the markup (KEEP-QUIRK of the original). 300 s without input or new lines fade the text out.
//  * Input: Enter sends through `ChatActions.submit` (history of 20, `Up` cycles it), `/` opens the command list above
//    the edit, `/r ` becomes `/whisper <last whisperer> `, a radio click prefills `/w `, `/s `, `/p `, `/g `.
//  * Skin: the layout tile plus the edit background of `WndChatEdit00_1/01/02.tga` painted after the controls.

import {
  APP_CHAT_FILTER,
  APP_COMMAND,
  APP_COMMUNICATION_CHAT,
  WIDC_BUTTON1,
  WIDC_CHECK1,
  WIDC_CHECK3,
  WIDC_CLOSE,
  WIDC_CUSTOM1,
  WIDC_EDIT,
  WIDC_HEAD,
  WIDC_HELP,
  WIDC_MACRO_CHAT,
  WIDC_MOVELOCK,
  WIDC_RADIO1,
  WIDC_RADIO2,
  WIDC_RADIO3,
  WIDC_RADIO4,
  WIDC_RADIO5,
  WIDC_STATIC1,
  WIDC_TEXT,
} from '../../gen/defs/resdata';
import { TID_ADMIN_ANNOUNCE, TID_APP_COMMUNICATION_CHAT, TID_SYS_HELP } from '../../gen/defs/text';
import type { Button } from '../controls/Button';
import { EditCtrl } from '../controls/EditCtrl';
import { TextCtrl } from '../controls/TextCtrl';
import {
  type FrameRects,
  type Point,
  type Rect,
  rectDeflate,
  rectHeight,
  rectWidth,
} from '../layout/geometry';
import {
  WBS_KEY,
  WBS_MODAL,
  WBS_MOVE,
  WBS_NODRAWFRAME,
  WBS_NOFRAME,
  WBS_THICKFRAME,
  WBS_VSCROLL,
  WTBID_CLOSE,
  WTBID_HELP,
  Wnm,
} from '../layout/styles';
import { MISSING, themePath } from '../skin/images';
import type { BakeSpec } from '../skin/baker';
import { lineHeight } from '../text/fonts';
import { lineText } from '../text/layout';
import { adjustMinRect } from '../wm/dragMath';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import type { Shortcut } from '../dnd/shortcut';
import { appendChatLine, opaque } from './appendLine';
import { ChatFilterWnd } from './ChatFilterWnd';
import { CommandWnd } from './CommandWnd';
import { CHAT_MIN_H, CHAT_MIN_W, chatGeometry, logWindowX } from './geometry';
import type { ChatHost } from './host';
import { MacroChatCtrl } from './MacroChatCtrl';
import { whisperTargetOfLine } from './ChatRouter';
import type { ChatTextTarget } from './ports';
import { CHANNELS, CHATSTY, tabPrefill } from './styles';

/** `MAX_HISTORY_STRING`. */
export const MAX_HISTORY_STRING = 20;
/** `m_timerInsMsg`: one help tip every 2 minutes. */
export const HELP_INTERVAL_MS = 120_000;
/** `m_timerInputTimeOut`: the text fades after 300 s without input or new lines. */
export const FADE_AFTER_MS = 300_000;
/** The colour of help lines: they do not reset the fade timer (`dwColor != 0xffffff00`). */
export const HELP_COLOR = 0xffffff00;

/** The edit of the chat window (`CWndEditChat`, WndCommunication.cpp:17-85). */
export class ChatEdit extends EditCtrl {
  readonly #chat: ChatWnd;
  constructor(chat: ChatWnd) {
    super();
    this.#chat = chat;
  }
  override onLButtonDown(): void {
    this.#chat.closeCommandList();
  }
  override onSetFocus(): void {
    this.#chat.prefillForTab(this);
    super.onSetFocus();
  }
  override onKillFocus(newWnd: WndBase | null): void {
    this.#chat.editLostFocus(newWnd);
    super.onKillFocus(newWnd);
  }
}

/** The text area: right click whispers to the speaker of the line, pointer events also reach the world. */
export class ChatText extends TextCtrl {
  readonly #chat: ChatWnd;
  constructor(chat: ChatWnd) {
    super();
    this.#chat = chat;
  }
  override onLButtonDown(pt: Point): void {
    this.#chat.forwardPointer('lbuttondown', this, pt);
  }
  override onLButtonUp(pt?: Point): void {
    super.onLButtonUp();
    if (pt) this.#chat.forwardPointer('lbuttonup', this, pt);
  }
  override onRButtonDown(pt: Point): void {
    this.#chat.forwardPointer('rbuttondown', this, pt);
    this.#chat.whisperToLineAt(pt);
  }
  override onRButtonUp(pt: Point): void {
    this.#chat.forwardPointer('rbuttonup', this, pt);
  }
  override onMouseMove(pt: Point): void {
    this.#chat.forwardPointer('mousemove', this, pt);
  }
}

export class ChatWnd extends WndNeuz implements ChatTextTarget {
  readonly appId = APP_COMMUNICATION_CHAT;
  /** `m_bChatLock`: the edit keeps the focus after Enter (persisted with the window rect). */
  chatLock = false;
  /** `m_bMoveLock`: the window cannot be moved (`WBS_MOVE` removed). */
  moveLock = false;
  /** `m_strHistory` (at most 20 lines, oldest first) and `m_nHistoryIndex`. */
  readonly history: string[] = [];
  historyIndex = 0;
  /** `m_strCharName`: the target of the last `/w` or `/whisper` (prefill of the whisper tab). */
  charName = '';
  edit!: ChatEdit;
  text!: ChatText;
  macro!: MacroChatCtrl;
  commandWnd: CommandWnd | null = null;
  filterWnd: ChatFilterWnd | null = null;
  readonly #host: ChatHost;
  #openingList = false;
  #fadeAt = 0;
  #helpAt = 0;
  #helpIndex = 0;
  #helpLines: readonly string[] = [];

  constructor(host: ChatHost) {
    super();
    this.#host = host;
  }

  // ---- creation ------------------------------------------------------------------------------------------------

  /** `Initialize`: `InitDialog(APP_COMMUNICATION_CHAT, WBS_KEY | WBS_THICKFRAME)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_KEY | WBS_THICKFRAME | extraStyle);
  }

  /** `CWndChat::SetWndRect`: the minimum size, client = window deflated `(4, 4, 6, 6)`, layout = client. */
  protected override computeRects(window: Rect): FrameRects {
    const w = adjustMinRect(window, this.rectWindowValue, CHAT_MIN_W, CHAT_MIN_H);
    const client = rectDeflate(w, 4, 4, 6, 6);
    return { window: w, client, layout: { ...client } };
  }

  /** `AdjustMinRect` while the user drags an edge. */
  override constrainResize(next: Rect, current: Rect): Rect {
    return adjustMinRect(next, current, CHAT_MIN_W, CHAT_MIN_H);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const now = this.env.host.now();
    this.#fadeAt = now + FADE_AFTER_MS;
    this.#helpAt = now + HELP_INTERVAL_MS;
    this.setTitle(this.#host.text(TID_APP_COMMUNICATION_CHAT) || this.getTitle());
    const view = this.#host.ports().view;

    // macro icon, edit and text area over the layout placeholders
    const macroRect = this.getWndCtrl(WIDC_MACRO_CHAT)?.rect ?? { l: 0, t: 0, r: 19, b: 20 };
    const editRect = this.getWndCtrl(WIDC_EDIT)?.rect ?? { l: 120, t: 110, r: 482, b: 124 };
    const textRect = this.getWndCtrl(WIDC_CUSTOM1)?.rect ?? { l: -4, t: 8, r: 508, b: 78 };
    this.macro = this.addControl(
      new MacroChatCtrl(() => this.edit.getString()),
      WIDC_MACRO_CHAT,
      macroRect,
      WBS_NODRAWFRAME,
    );
    this.edit = this.addControl(new ChatEdit(this), WIDC_EDIT, editRect, WBS_NODRAWFRAME);
    this.text = this.addControl(
      new ChatText(this),
      WIDC_TEXT,
      textRect,
      WBS_NOFRAME | WBS_NODRAWFRAME | WBS_VSCROLL,
    );
    const gm = view.isGm();
    this.text.setEnableClipboard(gm);
    this.text.setLineSpace(2);
    // `__CSC_VER11_1`: the edit is 90 px wider to the left, the macro icon sits 119 px left of the edit, the head
    // edit (the old speech-head) is disabled and hidden
    this.edit.setWndRect({ ...editRect, l: editRect.l - 90 }, false);
    this.macro.setWndRect(
      {
        l: editRect.l - 119,
        t: editRect.t,
        r: editRect.l - 119 + rectWidth(macroRect),
        b: editRect.t + rectHeight(macroRect),
      },
      false,
    );
    const head = this.getDlgItem(WIDC_HEAD);
    if (head) {
      head.enableWindow(false);
      head.setVisible(false);
    }
    this.#helpLines = this.#host.instantHelp() ? (view.helpLines?.() ?? []) : [];

    this.#radio(this.#host.router.currentTab)?.setCheck(true);
    this.setChannel();
    this.#button(WIDC_CHECK1)?.setCheck(this.chatLock);
    this.#button(WIDC_MOVELOCK)?.setCheck(this.moveLock);
    // bottom left of the layout rect
    const layout = this.env.wm.layoutRect;
    this.move({ x: 0, y: layout.b - rectHeight(this.rectWindowValue) });

    // the system log window exists (hidden) from now on, positioned next to the chat window
    const log = this.#host.ensureLogWindow();
    if (log) {
      if (!log.isVisible()) this.placeLogWindow(log);
      // the new frame took the focus; the chat window gets it back
      this.setFocus();
    }
    this.#button(WIDC_CHECK3)?.setCheck(log?.isVisible() ?? false);
    this.refreshControlState();
  }

  /** `OnSize`: every control is placed from the client rect. */
  override onSize(type: number, cx: number, cy: number): void {
    if (!this.isCreated) {
      super.onSize(type, cx, cy);
      return;
    }
    const g = chatGeometry(this.getClientRect(false));
    this.text.setWndRect(g.text, true);
    this.text.scrollToEnd();
    const head = this.getDlgItem(WIDC_HEAD);
    if (head) head.setWndRect({ ...head.rectWindow, t: g.headY.t, b: g.headY.b });
    this.edit.setWndRect({ ...this.edit.rectWindow, t: g.edit.t, b: g.edit.b, r: g.edit.r }, true);
    this.macro.setWndRect({ ...this.macro.rectWindow, t: g.macroY.t, b: g.macroY.b });
    CHANNELS.forEach((_, i) => {
      const at = g.radios[i];
      const radio = this.#radio(i);
      if (at && radio) radio.move(at);
    });
    this.#button(WIDC_CHECK1)?.move(g.lock);
    this.#button(WIDC_BUTTON1)?.move(g.filter);
    this.#button(WIDC_CHECK3)?.move(g.system);
    this.#button(WIDC_MOVELOCK)?.move(g.moveLock);
    this.#button(WIDC_HELP)?.move(g.help);
    this.#button(WIDC_CLOSE)?.move(g.close);
    this.getDlgItem(WIDC_STATIC1)?.move(g.imeStatic);
    super.onSize(type, cx, cy);
  }

  // ---- skin: the edit background -------------------------------------------------------------------------------

  /** The three images of the edit strip (`m_wndChatEdit[0..2]`). */
  static readonly EDIT_IMAGES = [
    'WndChatEdit00_1.tga',
    'WndChatEdit01.tga',
    'WndChatEdit02.tga',
  ] as const;

  override buildBakeSpec(): BakeSpec | null {
    const spec = super.buildBakeSpec();
    if (!spec) return null;
    const paths = ChatWnd.EDIT_IMAGES.map((n) => themePath(n));
    return {
      ...spec,
      afterImages: [
        ...(spec.afterImages ?? []),
        ...paths.map((path) => ({ path, mode: 'nibble' as const })),
      ],
      after: (ctx) => {
        spec.after?.(ctx);
        const [p0, p1, p2] = paths;
        const a = p0 ? ctx.image(p0, 'nibble') : undefined;
        const b = p1 ? ctx.image(p1, 'nibble') : undefined;
        const c = p2 ? ctx.image(p2, 'nibble') : undefined;
        if (!a || a === MISSING || !b || b === MISSING || !c || c === MISSING) return;
        if (!p0 || !p1 || !p2) return;
        // `AdditionalSkinTexture`: image 0 at the bottom left, then image 1 repeated, image 2 two steps before the end
        let x = 0;
        const y = spec.height - a.height;
        ctx.blit(p0, x, y, 'nibble');
        const n = Math.trunc((spec.width - a.width) / b.width);
        x += a.width;
        for (let i = 0; i < n; i += 1) {
          if (i === n - 2) ctx.blit(p2, x, y, 'nibble');
          else if (i < n - 2) ctx.blit(p1, x, y, 'nibble');
          x += b.width;
        }
      },
    };
  }

  // ---- the text area -------------------------------------------------------------------------------------------

  /** `CWndChat::PutString`: appends one line (the router decided that this window shows it). */
  put(text: string, color: number): void {
    if (color !== HELP_COLOR) this.#touch();
    appendChatLine(this.text, text, color);
  }

  /**
   * `SetChannel`: empties the area and refills it with the recorded lines of the current tab (`style & (etc |
   * filter[tab])`), markup NOT parsed (the codes are swallowed), then scrolls to the end.
   */
  setChannel(): void {
    const router = this.#host.router;
    const lines = router.lines(router.currentTab);
    this.text.setString('');
    this.text.addStrings(
      lines.map((l) => ({ text: `${l.text}\n`, color: opaque(l.color) })),
      false,
    );
    this.text.scrollToEnd();
  }

  /** Restores the text opacity and restarts the 300 s fade timer. */
  #touch(): void {
    this.#fadeAt = this.env.host.now() + FADE_AFTER_MS;
    this.text.setAlpha(255);
  }

  // ---- periodic work -------------------------------------------------------------------------------------------

  override process(now: number): void {
    super.process(now);
    this.#updateRadios();
    // the instant-help tips
    if (this.#helpLines.length > 0 && now >= this.#helpAt) {
      this.#helpAt = now + HELP_INTERVAL_MS;
      const prefix = this.#host.ports().view.helpPrefix?.() ?? this.#host.text(TID_SYS_HELP);
      const tip = this.#helpLines[this.#helpIndex % this.#helpLines.length] ?? '';
      this.#helpIndex = (this.#helpIndex + 1) % this.#helpLines.length;
      this.#host.router.putString(`${prefix} ${tip}`, null, HELP_COLOR, CHATSTY.HELP);
    }
    if (now >= this.#fadeAt) this.text.setAlpha(0);
    this.#rewriteReply();
    this.macro.setShown(this.edit.getString() !== '');
  }

  /** Radio 4 (party) and 5 (guild) follow the membership (`OnDraw`). */
  #updateRadios(): void {
    const view = this.#host.ports().view;
    this.#radio(3)?.enableWindow(view.inParty());
    this.#radio(4)?.enableWindow(view.inGuild());
  }

  /** `CWndChat::Process`: `/r ` with a known whisperer becomes `/whisper <name> `. */
  #rewriteReply(): void {
    const name = this.#host.ports().view.lastWhisperer();
    if (!name) return;
    const s = this.edit.getString();
    if (s === '/r ' || s === '/R ') {
      this.edit.setString(`/whisper ${name} `);
      this.edit.setFocus();
      this.edit.caretToEnd();
    }
  }

  /** Re-reads everything that depends on the ports (radio enabling, the macro icon). */
  refreshControlState(): void {
    this.#updateRadios();
    this.macro.setShown(this.edit.getString() !== '');
  }

  // ---- focus ---------------------------------------------------------------------------------------------------

  /** Enter (43): gives the edit the keyboard. Returns `false` when the window is not usable. */
  focusInput(): boolean {
    if (!this.isVisible()) return false;
    this.#touch();
    this.edit.setFocus();
    return true;
  }

  /** `CWndEditChat::OnSetFocus`: an empty edit gets the prefix of the current tab (unless the command list is open). */
  prefillForTab(edit: EditCtrl = this.edit): void {
    if (edit.getString() !== '' || this.commandWnd) return;
    const prefix = tabPrefill(this.#host.router.currentTab, this.#whisperName());
    if (prefix !== '') {
      edit.setString(prefix);
      this.macro.setShown(true);
    }
  }

  #whisperName(): string | null {
    return this.charName === '' ? null : this.charName;
  }

  /** `CWndEditChat::OnKillFocus`: the command list closes unless the focus went to it. */
  editLostFocus(newWnd: WndBase | null): void {
    const cmd = this.commandWnd;
    if (this.#openingList) return;
    if (cmd && newWnd !== this.edit && !cmd.contains(newWnd)) this.closeCommandList();
  }

  // ---- notifications -------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    this.#touch();
    let nid = id;
    switch (id) {
      case WIDC_CLOSE:
        nid = WTBID_CLOSE;
        break;
      case WIDC_HELP:
        nid = WTBID_HELP;
        break;
      case WIDC_BUTTON1:
        if (msg === Wnm.CLICKED) this.#openFilter();
        break;
      case WIDC_CHECK1:
        if (msg === Wnm.CLICKED) this.chatLock = (this.#button(WIDC_CHECK1)?.getCheck() ?? 0) !== 0;
        break;
      case WIDC_CHECK3:
        if (msg === Wnm.CLICKED) this.#toggleSystemLog();
        break;
      case WIDC_MOVELOCK:
        if (msg === Wnm.CLICKED)
          this.#setMoveLock((this.#button(WIDC_MOVELOCK)?.getCheck() ?? 0) !== 0);
        break;
      case WIDC_RADIO1:
      case WIDC_RADIO2:
      case WIDC_RADIO3:
      case WIDC_RADIO4:
      case WIDC_RADIO5:
        if (msg === Wnm.CLICKED)
          this.#selectTab(
            [WIDC_RADIO1, WIDC_RADIO2, WIDC_RADIO3, WIDC_RADIO4, WIDC_RADIO5].indexOf(id),
          );
        break;
      case APP_COMMAND:
        if (msg === Wnm.DBLCLK) this.#commandPicked();
        this.setFocus();
        break;
      case WIDC_EDIT:
        if (msg === Wnm.EN_UP) return this.#historyUp();
        if (msg === Wnm.EN_RETURN) this.#onReturn();
        else if (msg === Wnm.EN_CHANGE) this.#onEditChanged();
        break;
      default:
        break;
    }
    // the check boxes give the keyboard back to the world (the movement keys must work again)
    if (id === WIDC_CHECK1 || id === WIDC_CHECK3 || id === WIDC_MOVELOCK) this.#host.focusWorld();
    return super.onChildNotify(msg, nid, src);
  }

  #selectTab(tab: number): void {
    if (tab < 0) return;
    this.#host.router.currentTab = tab;
    this.edit.setString(tabPrefill(tab, null));
    this.macro.setShown(this.edit.getString() !== '');
    this.edit.setFocus();
    this.setChannel();
  }

  #setMoveLock(on: boolean): void {
    this.moveLock = on;
    if (on) this.delWndStyle(WBS_MOVE);
    else this.addWndStyle(WBS_MOVE);
  }

  // ---- system log window ---------------------------------------------------------------------------------------

  #toggleSystemLog(): void {
    const show = (this.#button(WIDC_CHECK3)?.getCheck() ?? 0) !== 0;
    const log = this.#host.ensureLogWindow();
    if (!log) return;
    if (show) this.placeLogWindow(log);
    log.setVisible(show);
  }

  /** Puts the log window right of the chat window when it fits in the layout rect, else left of it. */
  placeLogWindow(log: { rectClient: Rect; move(p: Point): void }): void {
    const layout = this.env.wm.layoutRect;
    const w = this.rectWindowValue;
    const x = logWindowX(w, rectWidth(log.rectClient), layout.r);
    log.move({ x, y: w.t });
  }

  /** The log window was hidden by its own menu: the toggle follows. */
  systemLogHidden(): void {
    this.#button(WIDC_CHECK3)?.setCheck(false);
  }

  // ---- filter window -------------------------------------------------------------------------------------------

  #openFilter(): void {
    if (this.filterWnd && !this.filterWnd.destroyed) return;
    const w = this.env.wm.open(APP_CHAT_FILTER, () => new ChatFilterWnd(this.#host, this));
    this.filterWnd = w instanceof ChatFilterWnd ? w : null;
  }

  /** `OnDestroyChildWnd(m_pWndChatFilter)`: forgets it and gives the keyboard back to the world. */
  filterClosed(w: ChatFilterWnd): void {
    if (this.filterWnd === w) this.filterWnd = null;
    this.#host.focusWorld();
  }

  // ---- command list --------------------------------------------------------------------------------------------

  /** `/` as the first character opens `APP_COMMAND` above the edit. */
  #openCommandList(): void {
    if (this.commandWnd) return;
    const names = this.#host.ports().commands.names();
    if (names.length === 0) return;
    const cmd = new CommandWnd(names, this.edit);
    cmd.onClosed = (w) => {
      this.commandClosed(w);
    };
    // creating the frame moves the focus around; the list may not close itself on the way
    this.#openingList = true;
    this.commandWnd = cmd;
    try {
      if (!cmd.initialize(this)) {
        this.commandWnd = null;
        return;
      }
    } finally {
      this.#openingList = false;
    }
    const at = this.edit.getScreenRect();
    cmd.move({ x: at.l, y: at.t - rectHeight(cmd.rectWindow) });
  }

  closeCommandList(): void {
    const cmd = this.commandWnd;
    this.commandWnd = null;
    if (cmd && !cmd.destroyed) cmd.destroy();
  }

  /** `OnDestroyChildWnd(m_pWndCommand)`. */
  commandClosed(w: CommandWnd): void {
    if (this.commandWnd === w) this.commandWnd = null;
  }

  /** Double click on a command: the edit becomes `"/" + name + " "`. */
  #commandPicked(): void {
    const cmd = this.commandWnd;
    if (!cmd) return;
    const name = cmd.selectedName();
    if (name !== null) {
      this.edit.setString(`/${name} `);
      this.edit.caretToEnd();
    }
    this.closeCommandList();
  }

  // ---- the edit ------------------------------------------------------------------------------------------------

  /** `EN_UP`: the history backwards, wrapping to the newest line. */
  #historyUp(): boolean {
    if (this.history.length === 0) return false;
    if (this.historyIndex >= this.history.length) this.historyIndex = 0;
    this.historyIndex += 1;
    this.edit.setString(this.history[this.history.length - this.historyIndex] ?? '');
    this.macro.setShown(this.edit.getString() !== '');
    return true;
  }

  #onEditChanged(): void {
    const s = this.edit.getString();
    this.macro.setShown(s !== '');
    const cmd = this.commandWnd;
    if (s !== '') {
      if (s.startsWith('/') && this.#caret() === 1 && !cmd) this.#openCommandList();
      else if (cmd) {
        const rest = s.slice(1);
        if (!rest.includes(' ')) cmd.list.selectString(0, rest);
        else this.closeCommandList();
      }
      if (s === '/r ' || s === '/R ') this.#rewriteReply();
    } else if (cmd) {
      this.closeCommandList();
    }
  }

  /** `m_wndEdit.GetOffset()`: the caret position (the text length without a DOM element). */
  #caret(): number {
    const el = this.edit.element;
    return el ? (el.selectionStart ?? el.value.length) : this.edit.getString().length;
  }

  /** `EN_RETURN`. */
  #onReturn(): void {
    let finished = true;
    const cmd = this.commandWnd;
    if (cmd) {
      const typed = this.edit.getString().slice(1);
      const picked = cmd.selectedName();
      if (picked !== null && picked !== typed) {
        this.edit.setString(`/${picked} `);
        this.edit.caretToEnd();
        finished = false;
      }
      this.closeCommandList();
    }
    if (finished) this.#submit();
  }

  /** Sends the line (history, `ChatActions.submit`, whisper partner) and hands the keyboard back. */
  #submit(): void {
    const line = this.edit.getString();
    if (line !== '') {
      if (this.history.length >= MAX_HISTORY_STRING) this.history.shift();
      this.history.push(line);
    }
    this.historyIndex = 0;
    this.#host.ports().actions.submit(line);
    const m = /^\/\s*(w|whisper)\s+(\S+)/.exec(line);
    if (m?.[2]) this.charName = m[2];
    this.edit.empty();
    this.macro.setShown(false);
    if (!this.chatLock) {
      if (this.isWndStyle(WBS_MODAL)) this.env.wm.endChatOverModal();
      else this.#host.focusWorld();
    } else {
      this.edit.setFocus();
    }
  }

  // ---- pointer -------------------------------------------------------------------------------------------------

  override onLButtonDown(pt?: Point): void {
    this.closeCommandList();
    if (pt) this.forwardPointer('lbuttondown', this, pt);
  }
  override onLButtonUp(pt?: Point): void {
    if (pt) this.forwardPointer('lbuttonup', this, pt);
  }
  override onRButtonDown(pt?: Point): void {
    if (pt) this.forwardPointer('rbuttondown', this, pt);
  }
  override onRButtonUp(pt?: Point): void {
    if (pt) this.forwardPointer('rbuttonup', this, pt);
  }
  override onMouseMove(pt?: Point): void {
    if (pt) this.forwardPointer('mousemove', this, pt);
  }

  /** The pointer events of the chat window also reach the world (a click through the chat still moves the character). */
  forwardPointer(
    kind: 'lbuttondown' | 'lbuttonup' | 'rbuttondown' | 'rbuttonup' | 'mousemove',
    source: WndBase,
    pt: Point,
  ): void {
    const view = this.#host.ports().view;
    const o = source.getScreenClientRect();
    view.worldPointer?.(kind, { x: pt.x + o.l, y: pt.y + o.t });
  }

  /** `OnRButtonDown`: a right click on a chat line starts a whisper to its speaker. */
  whisperToLineAt(pt: Point): void {
    const s = this.text.state.get();
    const pitch = lineHeight(this.text.font) + s.lineSpace;
    const first =
      this.text.scrollBar && (this.text.style & WBS_VSCROLL) !== 0
        ? this.text.scrollBar.getScrollPos()
        : 0;
    const row = Math.floor(pt.y / pitch);
    if (pt.y < 0 || row < 0) return;
    const line = s.lines[first + row];
    if (!line) return;
    const view = this.#host.ports().view;
    const name = whisperTargetOfLine(lineText(line), view.selfName(), this.#announceTag());
    if (name) {
      this.edit.setString(`/whisper ${name} `);
      this.edit.setFocus();
      this.edit.caretToEnd();
      this.macro.setShown(true);
    }
  }

  /** The name inside the announcement tag `TID_ADMIN_ANNOUNCE` (`"[Notice]"` -> `Notice`). */
  #announceTag(): string {
    const t = this.#host.text(TID_ADMIN_ANNOUNCE).replace('[', '');
    const i = t.indexOf(']');
    // the English `TID_ADMIN_ANNOUNCE` is a bare `%s`: no bracket, so no announcement is excluded (as in the original)
    return i > 0 ? t.slice(0, i) : '';
  }

  // ---- persistence and teardown --------------------------------------------------------------------------------

  /** `SerializeRegInfo`: version 1 stores the chat lock. */
  override serializeRegInfo(): unknown {
    return { chatLock: this.chatLock };
  }
  override restoreRegInfo(v: unknown, version: number): void {
    if (version < 1 || typeof v !== 'object' || v === null) return;
    const lock = (v as { chatLock?: unknown }).chatLock;
    if (typeof lock === 'boolean') {
      this.chatLock = lock;
      this.#button(WIDC_CHECK1)?.setCheck(lock);
    }
  }

  override onDestroy(): void {
    this.closeCommandList();
    const f = this.filterWnd;
    this.filterWnd = null;
    if (f && !f.destroyed) f.destroy();
    super.onDestroy();
  }

  // ---- helpers -------------------------------------------------------------------------------------------------

  #button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }

  /** The radio of a tab (0..4). */
  #radio(tab: number): Button | null {
    const ids = [WIDC_RADIO1, WIDC_RADIO2, WIDC_RADIO3, WIDC_RADIO4, WIDC_RADIO5];
    const id = ids[tab];
    return id === undefined ? null : this.#button(id);
  }
}
