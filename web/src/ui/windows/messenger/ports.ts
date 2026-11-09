// Ports of the messenger windows (Plan/42 §19.4 I, WP-UI-43, WP-UI-44; wired by 45 WP-CFEAT-39, -40). The READ MODELS (`MessengerView`,
// `CampusView`, `ImView`, `BlockListView`: read through a `ViewSource`, diffed by the shells) and the ACTIONS the shells call; the dialogs take
// one request object each (the texts and the callbacks), like the party and duel windows. Plain TS interfaces: `ui/` never imports `features/`.
// Every action names the legacy send behind it; the decisions (gates, boxes, lines, sends) are the feature's, the shells draw and forward.

import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** `FRS_*` (`CRTMessenger` states, `src:_Common/messenger.h`). */
export const FRS = {
  ONLINE: 0,
  OFFLINE: 1,
  BLOCK: 2,
  ABSENT: 3,
  HARDPLAY: 4,
  EAT: 5,
  REST: 6,
  MOVE: 7,
  DIE: 8,
  DANGER: 9,
  OFFLINEBLOCK: 10,
  AUTOABSENT: 11,
} as const;
/** `MAX_FRIENDSTAT`: the states 0..11 are valid. */
export const MAX_FRIENDSTAT = 12;

/** The two list pages of the messenger (the campus page has its own shell). */
export type MessengerPage = 'friend' | 'guild';

/** One line of a list page: the facts of a character (the cache and the friend record), already joined by the feature. */
export interface MessengerRow {
  /** The character id. */
  readonly id: number;
  readonly name: string;
  readonly level: number;
  /** The job id (`JOB_*`). */
  readonly job: number;
  /** `JTYPE_*` of the job (`prj.m_aJob[ job ].dwJobType`): base 0, expert 1, pro 2, master 5, hero 6. */
  readonly jobType: number;
  /** The job name: the tooltip of the job icon. */
  readonly jobName: string;
  /** `FRS_*`; the sort counts `OFFLINE` as 12. */
  readonly state: number;
  /** The channel: 100 for an offline friend, else the `login` of the cache (1..10 draw a digit). */
  readonly channel: number;
  /** The friend is blocked by me (a red name). */
  readonly blocked: boolean;
  /** Housing (M7): the friend may visit my house (a green name and level). Absent = no housing. */
  readonly visitAllowed?: boolean;
}

/** What the messenger window shows. */
export interface MessengerView {
  /** `m_RTMessenger.GetState()`. */
  readonly myState: number;
  /** `g_pPlayer->GetName( TRUE )`. */
  readonly myName: string;
  /** The server name at (170, 13); one server and one channel in this build. */
  readonly serverName: string;
  /** `g_pPlayer->GetCampusPoint()`, drawn on the campus tab. */
  readonly campusPoints: number;
  /** `MODE_MAILBOX`: the mail icon flashes. */
  readonly mailFlash: boolean;
  /** `m_nTagCount > 0 && !m_bCheckTag`: the notes button flashes. */
  readonly noteFlash: boolean;
  readonly friends: readonly MessengerRow[];
  readonly guild: readonly MessengerRow[];
}

export const EMPTY_MESSENGER_VIEW: MessengerView = {
  myState: FRS.ONLINE,
  myName: '',
  serverName: '',
  campusPoints: 0,
  mailFlash: false,
  noteFlash: false,
  friends: [],
  guild: [],
};

/** The ids of the friend menu (`CWndFriendCtrlEx::OnCommand`). */
export const FRIEND_MENU = {
  MESSAGE: 0,
  BLOCK: 1,
  DELETE: 2,
  MOVE: 3,
  INVITE_PARTY: 4,
  NOTE: 6,
  PROHIBIT_VISIT: 7,
  ALLOW_VISIT: 8,
} as const;

/** One entry of the friend menu: the C++ command id and its text. */
export interface FriendMenuEntry {
  readonly id: number;
  readonly text: string;
}

export interface MessengerActions {
  /** The state menu: `SendSetState( nID )`, 0..11. */
  setState(state: number): void;
  /** `SendGetFriendState()` (the window opens, the friend tab is selected); the feature skips an empty list. */
  refreshFriends(): void;
  /** `WIDC_ADD`: `APP_ADDFRIEND` (opens once). */
  openAddFriend(): void;
  /** `WIDC_BUTTON2`: `APP_MESSENGER_NOTE` (opens once, `m_bCheckTag = TRUE`). */
  openNotes(): void;
  /** A double click on a row: the IM window of an online friend, else the line `TID_GAME_NOTLOGIN` / `TID_GAME_MSGBLOCKCHR`. */
  activate(page: MessengerPage, id: number): void;
  /** The entries of the right-click menu of a friend, in the order of the original. */
  friendMenu(id: number): readonly FriendMenuEntry[];
  /** An entry of the friend menu was picked (`CWndFriendCtrlEx::OnCommand`). */
  pickFriendMenu(id: number, entry: number): void;
  /** A click beside the rows: `g_WorldMng()->SetObjFocus( NULL )`. */
  clearTarget(): void;
}

export interface MessengerPorts {
  readonly view: ViewSource<MessengerView>;
  readonly actions: MessengerActions;
}

// ---- the campus tab -------------------------------------------------------------------------------------------------------

/** The campus page: the master (when the own character is a pupil) or the disciples (when it is the master). */
export interface CampusView {
  readonly role: 'none' | 'master' | 'pupil';
  /** The master of a pupil. */
  readonly master: MessengerRow | null;
  /** The disciples of a master. */
  readonly disciples: readonly MessengerRow[];
}

export const EMPTY_CAMPUS_VIEW: CampusView = { role: 'none', master: null, disciples: [] };

export interface CampusActions {
  /** The menu entry `TID_GAME_MENU_CAMPUS_SEVERANCE` on a row: `CWndCampusSeveranceConfirm( id, name )`. */
  severance(id: number): void;
}

export interface CampusTabPorts {
  readonly view: ViewSource<CampusView>;
  readonly actions: CampusActions;
}

// ---- the IM window ---------------------------------------------------------------------------------------------------------

/** One line of the conversation (`CWndMessage::AddMessage`): mine in red, the partner's in blue. */
export interface ImLine {
  readonly mine: boolean;
  readonly from: string;
  readonly text: string;
}

export interface ImView {
  /** The partner's name (the title carries it; a rename changes it). */
  readonly partner: string;
  readonly lines: readonly ImLine[];
  /** The partner is blocked by me: the edit is hidden and Send disabled. */
  readonly blocked: boolean;
}

/** `CWndMessage` (`APP_MESSAGE`): one window per partner. */
export interface ImRequest {
  readonly view: ViewSource<ImView>;
  /** The limit of the edit: `gameplay.chat_max_chars` minus the `/say "<name>" ` prefix. */
  readonly maxChars: number;
  /** Enter in the edit or Send with a non-empty text: `SendChat( "/say \"name\" text" )`. */
  send(text: string): void;
  closed?(): void;
}

// ---- the popups -------------------------------------------------------------------------------------------------------------

/** One line of a popup: `head` is rich text (`#cff0000ff<from> :#nc`), `body` is the sender's words, shown as they are (no markup). */
export interface InstantLine {
  readonly head: string;
  readonly body: string;
}

/** `CWndInstantMsg` (`APP_INSTANTMSG`): a popup that lives `ttlMs` after its last line. */
export interface InstantRequest {
  /** The partner's name, or `Mail` / `Event`: the key of the popup (one per key). */
  readonly key: string;
  /** The first line. */
  readonly line: InstantLine;
  readonly ttlMs: number;
  /** A click on the popup (not on its close button): `true` closes the popup. */
  click?(): boolean;
  closed?(): void;
}

// ---- the dialogs ---------------------------------------------------------------------------------------------------------------

/** `CWndFriendConFirm` (`APP_FRIEND_CONFIRM`). */
export interface FriendConfirmRequest {
  /** `TID_DIAG_0071( name )`. */
  readonly text: string;
  /** Yes: `SendAddFriend`. */
  yes(): void;
  /** No: `SendFriendCancel`. */
  no(): void;
  closed?(): void;
}

/** `CWndAddFriend` (`APP_ADDFRIEND`). */
export interface AddFriendRequest {
  /** OK with the typed name; `clear` keeps the window with an empty edit, `close` closes it. */
  submit(name: string): 'clear' | 'close';
  closed?(): void;
}

/** `CWndMessageNote` (`APP_MESSAGE_NOTE`): a note for an offline friend. */
export interface NoteRequest {
  readonly name: string;
  /** Send: the byte rule and `SendMessageNote`; the window closes either way. */
  send(text: string): void;
  closed?(): void;
}

/** One undelivered note. */
export interface NoteEntry {
  readonly from: string;
  /** `YYYYMMDD`. */
  readonly date: number;
  readonly text: string;
}

/** `CWndMessengerNote` (`APP_MESSENGER_NOTE`): the notes of the join. */
export interface NotesRequest {
  readonly notes: readonly NoteEntry[];
  closed?(): void;
}

/** `CWndSummonFriend` (`APP_SUMMON_FRIEND`): the name of the friend to summon. */
export interface SummonFriendRequest {
  /** The online, unblocked friends of the combo box (the edit accepts any text). */
  readonly friends: readonly string[];
  /** OK with the typed text; `false` keeps the window (the name is too short or too long). */
  submit(name: string): boolean;
  closed?(): void;
}

/** `CWndSummonFriendMsg` (`APP_MESSAGEBOX_SUMMONFRINED`): the friend's question. */
export interface SummonFriendMsgRequest {
  /** `TID_DIAG_SUMMONFRIEND_CONFIRM( summoner, world )`. */
  readonly text: string;
  /** OK: `SendSummonFriendConfirm`. */
  ok(): void;
  /** Cancel: `SendSummonFriendCancel`. */
  cancel(): void;
  closed?(): void;
}

/** `CWndChattingBlockingList` (`APP_CHATTING_BLOCKING_LIST`): the ignore list, read only. */
export interface BlockListView {
  /** The window title: `TID_GAME_CHATTING_BLOCKING_LIST_TITLE( own name )`. */
  readonly title: string;
  /** The group title: `TID_GAME_CHATTING_BLOCKING_USERS_TOTAL_NUMBER( n )`. */
  readonly total: string;
  /** The names in stored order. */
  readonly names: readonly string[];
}

export interface BlockListRequest {
  readonly view: ViewSource<BlockListView>;
  closed?(): void;
}
