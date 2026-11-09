// `QuestTree` (`CWndQuestTreeCtrl`, `CWndQITreeCtrl`; WP-UI-23): the quest tree of the quest window and of the quick info
// window as a `TreeCtrl` with the two additions of the originals:
//   * CHECK BOXES (`m_bMemberCheckingMode`): every quest leaf has a check box between the open / close icon and the title;
//     ticking one registers the quest for the quick info window, at most `maxChecked` (5, `MAX_CHECK_NUMBER`) at a time: a sixth
//     tick is refused and `delegate.rejected` runs (the shell prints `TID_GAME_ERROR_DONT_CHECK`); `delegate.checked` runs for
//     every accepted change (the shell sends `CheckedQuestId`). A check box of a hidden row (its folder is closed) does not exist;
//   * STATE COLOURS: a node has a quest state and the title takes the colour of the state (`locked` is the pale grey 0xffc0c0c0
//     of a quest the player cannot take yet, `complete` green, `failed` red, `active` blue, `normal` the tree's dark grey), the
//     selected row keeps the tree's select colour.
// Everything else (open / close, selection, DBLCLK, wheel, the docked scroll bar, the geometry) is the inherited tree.

import type { Point, Rect } from '../layout/geometry';
import { TreeCtrl, type TreeNode, TREE_COLOR } from '../controls/TreeCtrl';
import { viewOf } from '../controls/bind';
import { QuestTreeView } from './QuestTreeView';

export type QuestState = 'normal' | 'active' | 'complete' | 'failed' | 'locked';

/** The title colours of the quest states (`0xAARRGGBB`). */
export const QUEST_STATE_COLORS: Readonly<Record<QuestState, number>> = {
  normal: TREE_COLOR,
  active: 0xff0000ff,
  complete: 0xff008000,
  failed: 0xffff0000,
  locked: 0xffc0c0c0,
};

/** `CWndQuest::MAX_CHECK_NUMBER`. */
export const MAX_CHECKED_QUESTS = 5;

/** The check box: 13 x 13 between the icon (8 px wide) and the keyword. */
export const CHECK_SIZE = 13;
export const CHECK_X = 11;
/** The keyword starts here when the tree has check boxes (`m_nCategoryTextSpace`). */
export const CHECK_TEXT_SPACE = 30;

export interface QuestTreeDelegate {
  /** An accepted tick change of a quest (`data` = the quest id). */
  checked?(node: TreeNode, checked: boolean): void;
  /** A tick that was refused because `maxChecked` quests are ticked already. */
  rejected?(node: TreeNode): void;
}

export interface QuestTreeOptions {
  /** The tree shows check boxes on its quest leaves. */
  readonly checkable?: boolean;
  readonly maxChecked?: number;
}

export class QuestTree extends TreeCtrl {
  #delegate: QuestTreeDelegate;
  readonly checkable: boolean;
  readonly maxChecked: number;
  /** The ticked quests by `data`. */
  readonly #checked = new Set<number>();
  readonly #states = new Map<number, QuestState>();
  /** The nodes without a check box. */
  readonly #forbidden = new Set<number>();

  constructor(options: QuestTreeOptions = {}, delegate: QuestTreeDelegate = {}) {
    super();
    this.#delegate = delegate;
    this.checkable = options.checkable ?? true;
    this.maxChecked = options.maxChecked ?? MAX_CHECKED_QUESTS;
    if (this.checkable) this.categoryTextSpace = CHECK_TEXT_SPACE;
    this.view = viewOf<QuestTree>(QuestTreeView);
  }

  setDelegate(delegate: QuestTreeDelegate): void {
    this.#delegate = delegate;
  }

  // ---- states ------------------------------------------------------------------------------------------------

  /** Sets the quest state of a node (its title colour follows). */
  setState(node: TreeNode, state: QuestState): void {
    this.#states.set(node.id, state);
    node.color = QUEST_STATE_COLORS[state] >>> 0;
    this.#touch();
  }

  stateOf(node: TreeNode): QuestState {
    return this.#states.get(node.id) ?? 'normal';
  }

  // ---- check boxes -------------------------------------------------------------------------------------------

  isChecked(node: TreeNode): boolean {
    return this.#checked.has(node.data);
  }

  get checkedCount(): number {
    return this.#checked.size;
  }

  /** The ticked quest ids (`data`), in tick order. */
  checkedIds(): readonly number[] {
    return [...this.#checked];
  }

  /** The shell restores the ticks it got from the server (`IsCheckedQuestID`): no delegate call. */
  setCheckedIds(ids: readonly number[]): void {
    this.#checked.clear();
    for (const id of ids.slice(0, this.maxChecked)) this.#checked.add(id);
    this.#touch();
  }

  /** Whether a node carries a check box: a quest leaf that is not forbidden (`bForbidChecking`: the leaf of a completed quest). */
  hasCheckBox(node: TreeNode): boolean {
    return this.checkable && node.children.length === 0 && !this.#forbidden.has(node.id);
  }

  /** `InsertItem( ..., bForbidChecking )`: the leaf of this node gets no check box (a completed quest). */
  forbidChecking(node: TreeNode, forbid = true): void {
    if (forbid) this.#forbidden.add(node.id);
    else this.#forbidden.delete(node.id);
    this.#touch();
  }

  /** The check box rect of a row in client coordinates. */
  checkRect(rowIcon: Rect): Rect {
    const l = rowIcon.l + CHECK_X;
    const t = rowIcon.t - Math.floor((CHECK_SIZE - 9) / 2);
    return { l, t, r: l + CHECK_SIZE, b: t + CHECK_SIZE };
  }

  /**
   * Toggles the tick of a quest node: ticking needs a free place (`maxChecked`), a refused tick leaves everything as it was
   * and calls `rejected`. Returns whether the state changed.
   */
  toggleCheck(node: TreeNode): boolean {
    if (!this.hasCheckBox(node)) return false;
    if (this.#checked.has(node.data)) {
      this.#checked.delete(node.data);
      this.#touch();
      this.#delegate.checked?.(node, false);
      return true;
    }
    if (this.#checked.size >= this.maxChecked) {
      this.#delegate.rejected?.(node);
      return false;
    }
    this.#checked.add(node.data);
    this.#touch();
    this.#delegate.checked?.(node, true);
    return true;
  }

  #touch(): void {
    this.state.patch({ version: this.state.get().version + 1 });
  }

  // ---- pointer -----------------------------------------------------------------------------------------------

  override onLButtonDown(pt: Point): void {
    if (this.checkable) {
      for (const row of this.rows()) {
        if (!this.hasCheckBox(row.node)) continue;
        const r = this.checkRect(row.icon);
        if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) {
          this.toggleCheck(row.node);
          return;
        }
      }
    }
    super.onLButtonDown(pt);
  }
}
