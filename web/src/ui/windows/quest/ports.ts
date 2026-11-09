// The requests and ports of the quest windows (Plan/42 §19.4 G, 45 §28.1; WP-UI-53): the windows are shells, every decision goes through ONE request object that carries what the window shows and
// the callbacks of its buttons. 45 builds the requests (`features/quest/questModule.ts`, `features/events/...` for the tower); nothing here knows a store or a socket.
//
// The send table (legacy name -> the request member that ends in it; rule C9 markers sit in 45's senders):
//
//   SendCheckedQuestId( id, check )      QuestListPorts.tick           a tick or untick of a quest leaf of the list
//   SendRemoveQuest( id )                QuestListPorts.remove         OK of the delete confirmation of the list
//   SendReqNPCPos( key )                 QuestDetailRequest.clickLine  a click on the "go to <NPC>" line of the condition tree (and the quick info's)
//   SendTeleportToHeavenTower( floor )   HeavenTowerRequest.enter      OK of the entrance confirmation, floors 1..5
//
// The windows that react to the book (the list, the detail, the quick info) call `subscribe( fn )` of their request and read the view again when it fires.

/** A node of the quest list (`TREEELEM`): a folder or a quest leaf. */
export interface QuestTreeNodeView {
  /** The quest id (`m_dwData`). */
  readonly id: number;
  /** The keyword: the title, plus `"(state, id)"` for a game master. */
  readonly label: string;
  /** An active or completed quest (a plain folder is not). */
  readonly isQuest: boolean;
  readonly completed: boolean;
  readonly children: readonly QuestTreeNodeView[];
}

/** A line of a condition tree: the markup text, the goal index of a click (`m_dwData`), the colours. */
export interface QuestLineView {
  readonly text: string;
  readonly data: number;
  readonly color: number;
  readonly selectColor: number;
  /** The "go to the NPC" line (`TID_QUEST_DESTINATION` of the end character): a click also asks for the NPC position. */
  readonly role?: 'character';
}

/** What the quest windows read of the client besides their request: texts and the standard boxes. */
export interface QuestHost {
  text(tid: number): string;
  /** `GETTEXT` with the arguments applied. */
  fmt(tid: number, ...args: (string | number)[]): string;
  /** `OpenMessageBox( text )`. */
  box(text: string): void;
  /** `OpenMessageBoxWithTitle( title, text )`. */
  boxWithTitle(title: string, text: string): void;
  /** A box with OK / Cancel: OK runs `onOk`, Cancel only closes. `owner` receives the command (the Heaven Tower). */
  confirm(text: string, onOk: () => void, owner?: unknown): void;
}

// ---- the list --------------------------------------------------------------------------------------------------------------------------------

/** The state of one quest as the Remove button reads it. */
export interface QuestStateFacts {
  readonly active: boolean;
  readonly completed: boolean;
  readonly state: number;
  /** `SetRemove( 0 )`. */
  readonly noRemove: boolean;
}

/** The ports of the list applet (`APP_QUEST_EX_LIST`), installed with `ui.quest.enableApplet( ports )`. */
export interface QuestListPorts {
  /** Calls `fn` whenever anything the list shows changed; returns the unsubscribe. */
  subscribe(fn: () => void): () => void;
  /** The title of a tab: the property of the kind id, `''` for none (the window then uses the English fallback). */
  tabTitle(tab: number): string;
  /** The four trees of a group (`QUEST_GROUP`: 0 all, 1 current, 2 complete). */
  tree(group: number): readonly (readonly QuestTreeNodeView[])[];
  /** The ticked quests (`m_aCheckedQuest`, the store's count). */
  checked(): readonly number[];
  /** The memory of the window (`CTreeInformationManager::m_nSelectedTabNumber` and `m_eQuestListGroup`). */
  memory(): { readonly tab: number; readonly group: number };
  setMemory(tab: number, group: number): void;
  /** `IsAuthHigher( AUTH_GAMEMASTER )`. */
  isGm(): boolean;
  /** `APP_DIALOG_EX` is open. */
  dialogOpen(): boolean;
  /** The facts of a quest id, `null` for an id the book does not know. */
  quest(questId: number): QuestStateFacts | null;
  /** A tick (`on`) or untick of a quest: the quick-tree entry and `SendCheckedQuestId`. */
  tick(questId: number, on: boolean): void;
  /** The tick was refused: `m_nCheckedQuestSize` is at the maximum (`TID_GAME_ERROR_DONT_CHECK` in its text colour). */
  tickRefused(): void;
  /** OK of the delete confirmation: `SendRemoveQuest` only if the quest is still active. */
  remove(questId: number): void;
  /** The detail request of a quest id. */
  detail(questId: number): QuestDetailRequest;
}

// ---- the detail ------------------------------------------------------------------------------------------------------------------------------

/** What the detail window shows of a quest: the description of the state, the condition lines, the reward text. */
export interface QuestDetailView {
  readonly description: string;
  readonly lines: readonly QuestLineView[];
  readonly reward: string;
}

export interface QuestDetailRequest {
  readonly questId: number;
  subscribe(fn: () => void): () => void;
  /** The texts now; `null` when the quest is neither active nor completed (the three texts are empty). */
  view(): QuestDetailView | null;
  /** A click on a condition line: the goal path of `SetQuestDestinationInformation` (and the NPC position request for the character line). */
  clickLine(line: QuestLineView): void;
  closed?(): void;
}

// ---- the quick info --------------------------------------------------------------------------------------------------------------------------

/** One ticked quest of the quick info: its title and its condition lines. */
export interface QuickQuestView {
  readonly id: number;
  readonly title: string;
  /** The node is open (`CTreeInformation::GetOpen`, default open). */
  readonly open: boolean;
  readonly lines: readonly QuestLineView[];
}

export interface QuestQuickInfoRequest {
  subscribe(fn: () => void): () => void;
  /** The ticked quests that have a definition, in the order of the list. */
  view(): readonly QuickQuestView[];
  /** `m_nCheckedQuestSize`: the window is visible while it is not 0 and grows when it grows. */
  checkedCount(): number;
  /** `CTreeInformation::SetOpen`. */
  setOpen(questId: number, open: boolean): void;
  /** A click on a quest title: the detail toggles and the destination is cleared. */
  clickTitle(questId: number): void;
  /** A click on a condition line: the goal path of the quest. */
  clickLine(questId: number, line: QuestLineView): void;
}

// ---- the Heaven Tower ------------------------------------------------------------------------------------------------------------------------

/** The Heaven Tower window's port (`ui.quest.openHeavenTower`): ui-secret-tax turns `enter` into `HeavenTowerTeleport`. */
export interface HeavenTowerRequest {
  /** OK of the entrance confirmation for the floors 1..5 (floor 6 and the row "Do not enter" send nothing). */
  enter(floor: 1 | 2 | 3 | 4 | 5): void;
  closed?(): void;
}
