// `WndQuestDetail` (`CWndQuestDetail`, `src:_Interface/WndQuest.cpp:1642-1885`, ref/22 §17 G, Plan/42 §19.4 G, 45 §28.1; WP-UI-53): `APP_QUEST_EX_DETAIL` (862, 272 x 416), the detail of one quest next to
// the list. The description of the state (`WIDC_TEXT_CONTENTS`), the condition tree (`WIDC_CUSTOM_CONDITION`, a `QuestConditionTree` made over the layout's rect: category space 0, VSCROLL) and the reward
// text (`WIDC_TEXT_REWARD`). The texts come from the request's view and are read again whenever the request says so (`UpdateQuestText`).
//
// Position: at `list.right - 2, list.top` and NOT movable while the list exists (the list moves it along, `WndQuest.setWndRect`), else centred and movable (`SetWndRect` of the original).

import {
  APP_QUEST_EX_DETAIL,
  APP_QUEST_EX_LIST,
  WIDC_CUSTOM_CONDITION,
  WIDC_TEXT_CONTENTS,
  WIDC_TEXT_REWARD,
} from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Rect } from '../../layout/geometry';
import { WBS_MOVE, WBS_VSCROLL } from '../../layout/styles';
import { WndNeuz } from '../../wnd/WndNeuz';
import { QuestConditionTree } from './QuestConditionTree';
import type { QuestDetailRequest } from './ports';

/** `0xff000000`: the colour of the description (`SetString( desc, 0xff000000 )`). */
const DESCRIPTION_COLOR = 0xff000000;

export class WndQuestDetail extends WndNeuz {
  readonly appId = APP_QUEST_EX_DETAIL;
  readonly #request: QuestDetailRequest;
  #tree: QuestConditionTree | null = null;
  #off: (() => void) | null = null;

  constructor(request: QuestDetailRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `GetQuestID()`. */
  get questId(): number {
    return this.#request.questId;
  }
  get description(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT_CONTENTS);
  }
  get reward(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT_REWARD);
  }
  get conditions(): QuestConditionTree | null {
    return this.#tree;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const at = this.getWndCtrl(WIDC_CUSTOM_CONDITION);
    if (at) {
      // the layout row is a text control; the original made the tree over the same rect and id
      const old = this.getDlgItem(WIDC_CUSTOM_CONDITION);
      if (old) this.removeControl(old);
      const tree = new QuestConditionTree();
      tree.onLine = (line) => {
        this.#request.clickLine(line);
      };
      this.addControl(tree, at.layout.id, at.rect, at.layout.style | WBS_VSCROLL);
      this.#tree = tree;
    }
    this.refresh();
    this.#off = this.#request.subscribe(() => {
      this.refresh();
    });
    const list = this.env.wm.get(APP_QUEST_EX_LIST);
    if (list) this.move({ x: list.rectWindow.r - 2, y: list.rectWindow.t });
    else this.moveParentCenter();
    this.#syncMovable();
  }

  /** `UpdateQuestText`: the three texts again. */
  refresh(): void {
    const v = this.#request.view();
    this.description?.setString(v?.description ?? '', DESCRIPTION_COLOR);
    this.#tree?.setLines(v?.lines ?? []);
    this.reward?.setString(v?.reward ?? '');
  }

  #syncMovable(): void {
    if (this.env.wm.get(APP_QUEST_EX_LIST)) this.delWndStyle(WBS_MOVE);
    else this.addWndStyle(WBS_MOVE);
  }

  /** `SetWndRect`: the window is fixed to the list while it exists. */
  override setWndRect(rect: Rect, onSize = false): void {
    if (this.isCreated) this.#syncMovable();
    super.setWndRect(rect, onSize);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
