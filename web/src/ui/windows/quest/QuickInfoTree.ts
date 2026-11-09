// `CWndQITreeCtrl` (`WndQuestQuickInfo.cpp:18-186`): the tree of the quick-info window - one category node per ticked quest (its title) with the condition lines below it. The tab width is 6 and the
// category space 12. A press on an icon opens or closes a node (reported to the window, which tells the quest's tree memory and grows the window), a press on a title or a line is reported with
// the quest id; the focused row takes the select colour. The fonts follow the hover fade of the window (`worldFont`: the outlined world font while the pointer is away).

import { TreeCtrl, type TreeNode } from '../../controls/TreeCtrl';
import type { Point } from '../../layout/geometry';
import { viewOf } from '../../controls/bind';
import type { QuestLineView } from './ports';
import { QuickInfoTreeView } from './QuickInfoTreeView';

export interface QuickPress {
  readonly kind: 'title' | 'line';
  readonly questId: number;
  readonly line?: QuestLineView;
}

export class QuickInfoTree extends TreeCtrl {
  readonly #lines = new Map<number, QuestLineView>();
  /** The outlined world font (`m_nAlphaCount == 0`). */
  worldFont = true;
  /** A folder was opened or closed by a press on its icon. */
  onToggle: ((node: TreeNode) => void) | null = null;
  /** A press on a title or a line. */
  onPress: ((press: QuickPress) => void) | null = null;

  constructor() {
    super();
    this.tabWidth = 6;
    this.categoryTextSpace = 12;
    this.view = viewOf<QuickInfoTree>(QuickInfoTreeView);
  }

  /** The font follows the hover fade. */
  setWorldFont(on: boolean): void {
    if (this.worldFont === on) return;
    this.worldFont = on;
    this.setFocusElem(this.getCurSel());
  }

  /** Replaces the tree: one category per quest, its lines below, open as remembered. */
  setQuests(
    quests: readonly {
      id: number;
      title: string;
      open: boolean;
      lines: readonly QuestLineView[];
    }[],
  ): void {
    this.deleteAllItems();
    this.#lines.clear();
    for (const q of quests) {
      const cat = this.insertItem(null, q.title, q.id);
      for (const line of q.lines) {
        const node = this.insertItem(cat, line.text, line.data);
        this.#lines.set(node.id, line);
      }
      this.setOpen(cat, q.open);
    }
  }

  /** The height of all visible rows (`GetTreeItemsMaxHeight`). */
  itemsHeight(): number {
    return 3 + this.rowCount() * this.pitch();
  }

  override onLButtonDown(pt: Point): void {
    for (const row of this.rows()) {
      const node = row.node;
      const icon = row.icon;
      if (
        node.children.length > 0 &&
        pt.x >= icon.l &&
        pt.x < icon.r &&
        pt.y >= icon.t &&
        pt.y < icon.b
      ) {
        this.setOpen(node, !node.open);
        this.onToggle?.(node);
        this.setFocusElem(node);
        continue;
      }
      const t = row.text;
      if (pt.x >= t.l && pt.x < t.r && pt.y >= t.t && pt.y < t.b) {
        if (node.children.length > 0 || node.parent === null) {
          this.onPress?.({ kind: 'title', questId: node.data });
        } else {
          const line = this.#lines.get(node.id);
          this.onPress?.({ kind: 'line', questId: node.parent.data, ...(line ? { line } : {}) });
        }
        this.setFocusElem(node);
      }
    }
  }

  /** The quick info runs its own presses; the base tree's CLICKED / DBLCLK notifications have no listener. */
  override onLButtonDblClk(): void {
    /* no double click behaviour in the original */
  }
}
