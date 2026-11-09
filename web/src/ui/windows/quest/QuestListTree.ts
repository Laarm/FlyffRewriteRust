// `CWndQuestTreeCtrl` (`WndQuest.cpp:1524-1602`): the tree of one tab of the quest list - `QuestTree` (check boxes, at most five) plus the report of a folder that a press on its icon opened or closed
// (`pWndQuest->ControlOpenTree( lpTreeElem )`, which records the open folders for the registration info). A double click is reported to the window by the notification `DBLCLK`.

import type { TreeNode } from '../../controls/TreeCtrl';
import type { Point } from '../../layout/geometry';
import { QuestTree } from '../../lists/QuestTree';

export class QuestListTree extends QuestTree {
  /** A folder was opened or closed by a press on its icon. */
  onToggle: ((node: TreeNode) => void) | null = null;

  override onLButtonDown(pt: Point): void {
    // the folder whose icon is under the pointer (a check box takes the press first, as in the base class)
    let hit: TreeNode | null = null;
    for (const row of this.rows()) {
      const r = row.icon;
      if (row.node.children.length > 0 && pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) {
        hit = row.node;
        break;
      }
    }
    const before = hit?.open;
    super.onLButtonDown(pt);
    if (hit && hit.open !== before) this.onToggle?.(hit);
  }
}
