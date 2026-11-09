// `MessengerWindowsService` (`ui.messenger`, Plan/42 §19.4 I; WP-UI-43, WP-UI-44): the messenger windows.
//
//   ui.messenger.setPorts({ messenger, campus })     45 WP-CFEAT-39, -40: the view and the actions of the messenger window and of its campus tab
//   (applet)  APP_MESSENGER_EX (hotkey E)            opens through the applet registry; the friend, guild and campus pages are its tab pages
//   ui.messenger.openMessage(request)                APP_MESSAGE: the IM window of ONE partner (`instanceKey` = the lower-cased name); a second open focuses it
//   ui.messenger.renameMessage(from, to)             a renamed partner: the window keeps its place under the new key (`CWndMgr::UpdateMessage`)
//   ui.messenger.openInstant(request)                APP_INSTANTMSG: a popup per key, stacked from the bottom right (`CWndMgr::OpenInstantMsg`)
//   ui.messenger.openFriendConfirm / openAddFriend / openNote / openNotes / openBlockList / openSummon / openSummonMsg      the dialogs
//
// The factories read the ports when a window is created, so ports installed later reach later windows.

import {
  APP_ADDFRIEND,
  APP_CHATTING_BLOCKING_LIST,
  APP_FRIEND_CONFIRM,
  APP_INSTANTMSG,
  APP_MESSAGE,
  APP_MESSAGE_NOTE,
  APP_MESSAGEBOX_SUMMONFRINED,
  APP_MESSENGER_EX,
  APP_MESSENGER_NOTE,
  APP_SUMMON_FRIEND,
} from '../../../gen/defs/resdata';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { MESSENGER_LAZY } from './lazy';
import {
  type AddFriendRequest,
  constantView,
  EMPTY_CAMPUS_VIEW,
  EMPTY_MESSENGER_VIEW,
  type BlockListRequest,
  type CampusTabPorts,
  type FriendConfirmRequest,
  type ImRequest,
  type InstantRequest,
  type MessengerActions,
  type MessengerPorts,
  type NoteRequest,
  type NotesRequest,
  type SummonFriendMsgRequest,
  type SummonFriendRequest,
} from './ports';
import type { WndAddFriend } from './WndAddFriend';
import type { WndBlockingList } from './WndBlockingList';
import { WndCampusTab } from './WndCampusTab';
import type { WndFriendConfirm } from './WndFriendConfirm';
import type { WndInstantMsg } from './WndInstantMsg';
import type { WndMessage } from './WndMessage';
import { imKey } from './imKey';
import { instantPosition } from './instantStack';
import type { WndMessageNote } from './WndMessageNote';
import { WndMessenger } from './WndMessenger';
import type { WndMessengerNote } from './WndMessengerNote';
import type { WndSummonFriend, WndSummonFriendMsg } from './WndSummonFriend';

export * from './ports';
export { WndMessenger, STATE_MENU, stateStatusText, TAB } from './WndMessenger';
export { WndCampusTab, MASTER_ROW_Y, DISCIPLE_ROW_Y } from './WndCampusTab';
export { MessengerListPage } from './MessengerListPage';
export { MessengerList, memberRowOf } from './MessengerList';
export type { WndMessage } from './WndMessage';
export type { WndInstantMsg } from './WndInstantMsg';
export type { WndMessageNote } from './WndMessageNote';
export type { WndMessengerNote } from './WndMessengerNote';
export type { WndAddFriend } from './WndAddFriend';
export type { WndFriendConfirm } from './WndFriendConfirm';
export type { WndBlockingList } from './WndBlockingList';
export type { WndSummonFriend, WndSummonFriendMsg } from './WndSummonFriend';
export * from './icons';
export * from './sort';
export * from './noteText';
export { imKey } from './imKey';
export { instantPosition, INSTANT_SIZE } from './instantStack';

export interface MessengerWindowDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

const NO_ACTIONS: MessengerActions = {
  setState: () => undefined,
  refreshFriends: () => undefined,
  openAddFriend: () => undefined,
  openNotes: () => undefined,
  activate: () => undefined,
  friendMenu: () => [],
  pickFriendMenu: () => undefined,
  clearTarget: () => undefined,
};

/** The ports before 45 installs the real ones: nobody on the lists, actions that do nothing. */
export const NULL_MESSENGER_PORTS: { messenger: MessengerPorts; campus: CampusTabPorts } = {
  messenger: { view: constantView(EMPTY_MESSENGER_VIEW), actions: NO_ACTIONS },
  campus: {
    view: constantView(EMPTY_CAMPUS_VIEW),
    actions: { severance: () => undefined },
  },
};

export class MessengerWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #atlases: UiAtlases;
  readonly #getText: (tid: number) => string;
  readonly #instants = new Map<string, WndInstantMsg>();
  #ports = NULL_MESSENGER_PORTS;

  constructor(deps: MessengerWindowDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#atlases = deps.atlases;
    this.#getText = deps.getText;
    deps.wm.applets.register({ app: APP_MESSENGER_EX, factory: () => this.createMessenger() });
  }

  /** Installs the ports of 45 (the rest keeps its previous value). */
  setPorts(ports: Partial<{ messenger: MessengerPorts; campus: CampusTabPorts }>): void {
    this.#ports = { ...this.#ports, ...ports };
  }

  get ports(): { messenger: MessengerPorts; campus: CampusTabPorts } {
    return this.#ports;
  }

  // ---- the applet -------------------------------------------------------------------------------------------------------------

  createMessenger(): WndMessenger {
    return new WndMessenger({
      ports: this.#ports.messenger,
      campus: () => this.#ports.campus,
      tooltip: this.#tooltip,
      atlases: this.#atlases,
      getText: this.#getText,
    });
  }

  messenger(): WndMessenger | null {
    const w = this.#wm.get(APP_MESSENGER_EX);
    return w instanceof WndMessenger ? w : null;
  }

  /** The campus page of the open messenger. */
  campusTab(): WndCampusTab | null {
    const w = this.messenger()?.campusPage;
    return w instanceof WndCampusTab ? w : null;
  }

  /** `UpdateCampusMemberList`: the campus page reads its view again. */
  refreshCampus(): void {
    this.campusTab()?.refresh();
  }

  // ---- the IM windows ---------------------------------------------------------------------------------------------------------

  /** `CWndMgr::OpenMessage`: opens the window of the partner in `request.view`, or focuses the one that is open. `null` while the chunk loads. */
  openMessage(request: ImRequest): WndMessage | null {
    const key = imKey(request.view.get().partner);
    const old = this.#wm.get(APP_MESSAGE, key);
    if (old && this.#wm.isDestroying(old)) this.#wm.flushDestroyed();
    const lazy = MESSENGER_LAZY.WndMessage;
    const open = (C: NonNullable<typeof lazy.ctor>): WndMessage | null =>
      this.#wm.open(APP_MESSAGE, () => new C(request, this.#getText), key);
    const ctor = lazy.ctor;
    if (ctor) return open(ctor);
    lazy.load().then(
      (c) => {
        open(c);
      },
      () => undefined,
    );
    return null;
  }

  /** `CWndMgr::GetMessage`. */
  message(name: string): WndMessage | null {
    const w = this.#wm.get(APP_MESSAGE, imKey(name));
    const ctor = MESSENGER_LAZY.WndMessage.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  /** `CWndMgr::UpdateMessage`: the window of `from` lives on under the key of `to`. Returns `false` when there is none (or `to` has one). */
  renameMessage(from: string, to: string): boolean {
    const ok = this.#wm.rekey(APP_MESSAGE, imKey(from), imKey(to));
    if (ok) this.message(to)?.setKey(imKey(to));
    return ok;
  }

  // ---- the popups ---------------------------------------------------------------------------------------------------------------

  /**
   * `CWndMgr::OpenInstantMsg`: the popup of a key; one per key (a second request returns the open one without touching its text). A new popup is placed by the number of
   * popups there already are (`nNumber`), from the bottom right of the layout upwards and then to the left.
   */
  openInstant(request: InstantRequest): WndInstantMsg | null {
    const old = this.#instants.get(request.key);
    if (old && this.#wm.isDestroying(old)) this.#wm.flushDestroyed();
    const existing = this.#instants.get(request.key);
    if (existing) return existing;
    const lazy = MESSENGER_LAZY.WndInstantMsg;
    const open = (C: NonNullable<typeof lazy.ctor>): WndInstantMsg | null => {
      const n = this.#instants.size;
      let created: WndInstantMsg | null = null;
      const w = this.#wm.open(
        APP_INSTANTMSG,
        () => {
          created = new C({
            ...request,
            closed: () => {
              for (const [k, v] of this.#instants) if (v === created) this.#instants.delete(k);
              request.closed?.();
            },
          });
          return created;
        },
        request.key,
      );
      if (!w) return null;
      this.#instants.set(request.key, w);
      const size = w.rectWindow;
      const at = instantPosition(n, this.#wm.layoutRect, {
        w: size.r - size.l,
        h: size.b - size.t,
      });
      w.move(at);
      return w;
    };
    const ctor = lazy.ctor;
    if (ctor) return open(ctor);
    lazy.load().then(
      (c) => {
        open(c);
      },
      () => undefined,
    );
    return null;
  }

  /** `CWndMgr::GetInstantMsg`. */
  instant(key: string): WndInstantMsg | null {
    return this.#instants.get(key) ?? null;
  }

  /** `m_mapInstantMsg.GetCount()`. */
  instantCount(): number {
    return this.#instants.size;
  }

  /** The keys of the open popups, oldest first. */
  instantKeys(): string[] {
    return [...this.#instants.keys()];
  }

  closeInstant(key: string): void {
    this.#instants.get(key)?.destroy();
  }

  /** `UpdateMessage` for the popup of a renamed player: it lives on under the new key. Returns `false` when there is none (or `to` has one). */
  renameInstant(from: string, to: string): boolean {
    const w = this.#instants.get(from);
    if (!w || this.#instants.has(to)) return false;
    if (!this.#wm.rekey(APP_INSTANTMSG, from, to)) return false;
    this.#instants.delete(from);
    this.#instants.set(to, w);
    w.setKey(to);
    return true;
  }

  // ---- the dialogs --------------------------------------------------------------------------------------------------------------

  /** Whether the window of an app is open and not on its way out. */
  #live(app: number): boolean {
    const w = this.#wm.get(app);
    return w !== null && !this.#wm.isDestroying(w);
  }

  /** Opens a dialog that exists once: `null` when it is open already. */
  #once<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    if (this.#live(app)) return null;
    this.#wm.flushDestroyed();
    return openLazy(this.#wm, app, lazy, build, () => undefined);
  }

  /** Opens a dialog that replaces the open one. */
  #replace<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      const old = this.#wm.get(a);
      if (!old) return;
      old.destroy();
      this.#wm.flushDestroyed();
    });
  }

  #typed<C extends WndClass>(app: number, lazy: LazyWnd<C>): InstanceType<C> | null {
    const w = this.#wm.get(app);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  /** `OnAddFriendReqest`: the confirm window; `null` when one is open (the caller answers the newcomer with `SendBlock( 6 )`). */
  openFriendConfirm(request: FriendConfirmRequest): WndFriendConfirm | null {
    return this.#once(APP_FRIEND_CONFIRM, MESSENGER_LAZY.WndFriendConfirm, (C) => new C(request));
  }
  isFriendConfirmOpen(): boolean {
    return this.#live(APP_FRIEND_CONFIRM);
  }
  /** The window goes without an answer. */
  closeFriendConfirm(): void {
    this.#wm.get(APP_FRIEND_CONFIRM)?.destroy();
  }
  friendConfirm(): WndFriendConfirm | null {
    return this.#typed(APP_FRIEND_CONFIRM, MESSENGER_LAZY.WndFriendConfirm);
  }

  /** `if( !g_WndMng.m_pWndAddFriend )`: the window opens once. */
  openAddFriend(request: AddFriendRequest): WndAddFriend | null {
    return this.#once(APP_ADDFRIEND, MESSENGER_LAZY.WndAddFriend, (C) => new C(request));
  }
  addFriend(): WndAddFriend | null {
    return this.#typed(APP_ADDFRIEND, MESSENGER_LAZY.WndAddFriend);
  }

  /** `SAFE_DELETE( m_pWndMessageNote ); new ...`: a new note window replaces the old one. */
  openNote(request: NoteRequest): WndMessageNote | null {
    return this.#replace(APP_MESSAGE_NOTE, MESSENGER_LAZY.WndMessageNote, (C) => new C(request));
  }
  note(): WndMessageNote | null {
    return this.#typed(APP_MESSAGE_NOTE, MESSENGER_LAZY.WndMessageNote);
  }

  /** `if( !g_WndMng.m_pWndMessengerNote )`: the notes window opens once. */
  openNotes(request: NotesRequest): WndMessengerNote | null {
    return this.#once(APP_MESSENGER_NOTE, MESSENGER_LAZY.WndMessengerNote, (C) => new C(request));
  }
  notes(): WndMessengerNote | null {
    return this.#typed(APP_MESSENGER_NOTE, MESSENGER_LAZY.WndMessengerNote);
  }

  /** `/ignorelist`: the block list opens once. */
  openBlockList(request: BlockListRequest): WndBlockingList | null {
    return this.#once(
      APP_CHATTING_BLOCKING_LIST,
      MESSENGER_LAZY.WndBlockingList,
      (C) => new C(request),
    );
  }
  blockList(): WndBlockingList | null {
    return this.#typed(APP_CHATTING_BLOCKING_LIST, MESSENGER_LAZY.WndBlockingList);
  }

  /** The use of the friend summon scroll: the name window replaces an open one. */
  openSummon(request: SummonFriendRequest): WndSummonFriend | null {
    return this.#replace(APP_SUMMON_FRIEND, MESSENGER_LAZY.WndSummonFriend, (C) => new C(request));
  }
  summon(): WndSummonFriend | null {
    return this.#typed(APP_SUMMON_FRIEND, MESSENGER_LAZY.WndSummonFriend);
  }

  /** The friend's question: one window at a time, a new request replaces the text. */
  openSummonMsg(request: SummonFriendMsgRequest): WndSummonFriendMsg | null {
    return this.#replace(
      APP_MESSAGEBOX_SUMMONFRINED,
      MESSENGER_LAZY.WndSummonFriendMsg,
      (C) => new C(request),
    );
  }
  summonMsg(): WndSummonFriendMsg | null {
    return this.#typed(APP_MESSAGEBOX_SUMMONFRINED, MESSENGER_LAZY.WndSummonFriendMsg);
  }
}
