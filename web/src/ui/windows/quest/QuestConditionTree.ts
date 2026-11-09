// `CWndQConditionTreeCtrl` (`WndQuest.cpp:1604-1640`): the condition tree of the detail window - lines without icons (`SetCategoryTextSpace( 0 )`), a click on a line runs the goal path of the quest
// (`SetQuestDestinationInformation` + `ProcessQuestDestinationWorldMap`). The window fills it from the request's lines; the click goes back through `onLine`.

import { TreeCtrl, type TreeNode } from '../../controls/TreeCtrl';
import type { Point } from '../../layout/geometry';
import type { QuestLineView } from './ports';

export class QuestConditionTree extends TreeCtrl {
  readonly #lines = new Map<number, QuestLineView>();
  /** A press on the text of a line. */
  onLine: ((line: QuestLineView) => void) | null = null;

  constructor() {
    super();
    this.categoryTextSpace = 0;
  }

  /** `DeleteAllItems` + `InsertItem` for every line (`data` = the goal index). */
  setLines(lines: readonly QuestLineView[]): void {
    this.deleteAllItems();
    this.#lines.clear();
    for (const line of lines) {
      const node = this.insertItem(null, line.text, line.data, line.color, line.selectColor);
      this.#lines.set(node.id, line);
    }
  }

  /** The line of a node. */
  lineOf(node: TreeNode): QuestLineView | undefined {
    return this.#lines.get(node.id);
  }

  /** The text rect of a line is the hit box (`pTreeItem->m_rect.PtInRect( point )`); the lines have no icon to toggle. */
  override onLButtonDown(pt: Point): void {
    for (const row of this.rows()) {
      const r = row.text;
      if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) {
        const line = this.#lines.get(row.node.id);
        if (line) this.onLine?.(line);
      }
    }
  }
}
