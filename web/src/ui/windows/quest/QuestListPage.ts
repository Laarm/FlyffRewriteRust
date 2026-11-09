// The page of one tab of the quest list: a frame without decoration that holds the `QuestListTree` of the tab over its whole client (`CWndQuestTreeCtrl` was the tab page itself in the original,
// created with `WBS_CHILD | WBS_NODRAWFRAME | WBS_VSCROLL`).

import { WBS_CHILD, WBS_NODRAWFRAME, WBS_NOFRAME, WBS_VSCROLL } from '../../layout/styles';
import { rectHeight, rectWidth } from '../../layout/geometry';
import type { TabCtrl } from '../../controls/TabCtrl';
import { CodePage } from '../char/CodePage';
import { QuestListTree } from './QuestListTree';

export class QuestListPage extends CodePage {
  readonly tree = new QuestListTree();

  override attach(
    tab: TabCtrl,
    id: number,
    style = WBS_CHILD | WBS_NOFRAME | WBS_NODRAWFRAME,
  ): boolean {
    return super.attach(tab, id, style);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.rectClient;
    this.addControl(
      this.tree,
      this.id + 100,
      { l: 0, t: 0, r: rectWidth(r), b: rectHeight(r) },
      WBS_VSCROLL | WBS_NODRAWFRAME,
    );
  }

  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    const r = this.rectClient;
    if (this.tree.isCreated)
      this.tree.setWndRect({ l: 0, t: 0, r: rectWidth(r), b: rectHeight(r) }, true);
  }
}
