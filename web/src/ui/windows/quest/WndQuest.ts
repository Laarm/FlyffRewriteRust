// `WndQuest` (`CWndQuest`, `src:_Interface/WndQuest.cpp:1-1602`, ref/22 §17 G, Plan/42 §19.4 G, 45 §28.1; WP-UI-53): the quest list applet `APP_QUEST_EX_LIST` (859, 336 x 416, keyword `WndQuest`). A tab
// control with four `QuestListTree` pages (Scenario 6000, Normal 6001, Request 6002, Event 6003; title centred; the tab title is the property of the kind id, else the English fallback), the combo
// ALL / CURRENT / COMPLETE (read-only, CURRENT by default), the Remove button and the close button. An APPLET: a plain class (the registry builds it), the ports come from the service.
//
//   * the trees follow the request's model on every change (deviation: the original rebuilt on dialog open/close and removals); folders start closed unless recorded in the open set; the folder icon and a
//     double click toggle a folder and record it (`ControlOpenTree`); a click on a leaf toggles the detail window (the same quest closes it, another replaces it) and refreshes the Remove button;
//   * a tick on a leaf: refused at five (`ports.tickRefused`, the box is unticked), else `ports.tick( id, true )`; an untick `ports.tick( id, false )`;
//   * Remove on a selected leaf closes the detail and opens the confirmation (250 x 130, `TID_GAME_QUEST_DELCONFIRM`, OK / Cancel); OK hands the id to `ports.remove` (which sends only for a quest that is
//     still active). Remove is enabled for a game master always (the original also asked the hidden `sunkist` option: Plan/45 §28.1 and 20 §12 item 7 are followed), else disabled for nothing selected,
//     a completed quest, state 14, `APP_DIALOG_EX` open and `no_remove`;
//   * a change of tab or group closes the detail; closing the list closes the detail; moving the list moves the detail along (`SetWndRect`);
//   * registration info `extra` v1: `{ selected, open: number[] }` (`SerializeRegInfo`).

import {
  APP_DIALOG_EX,
  APP_QUEST_EX_DETAIL,
  APP_QUEST_EX_LIST,
  WIDC_BUTTON_CLOSE,
  WIDC_COMBOBOX_QUEST_LIST,
  WIDC_QUEST_LIST_TABCTRL,
  WIDC_REMOVE,
} from '../../../gen/defs/resdata';
import { TID_GAME_QUEST_DELCONFIRM } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { ComboBox } from '../../controls/ComboBox';
import type { TabCtrl } from '../../controls/TabCtrl';
import type { TreeNode } from '../../controls/TreeCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import type { Rect } from '../../layout/geometry';
import { EBS_READONLY, Wnm } from '../../layout/styles';
import { COMPLETED_COLOR } from './colors';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { QuestListTree } from './QuestListTree';
import { QuestListPage } from './QuestListPage';
import type { QuestDetailRequest, QuestHost, QuestListPorts, QuestTreeNodeView } from './ports';
import { QUEST_GROUP_TIDS, QUEST_TAB_FALLBACK, QUEST_TAB_KINDS } from './rows';

/** What the applet needs besides its ports: the host and the detail window of the service. */
export interface QuestListDeps {
  readonly host: QuestHost;
  readonly ports: () => QuestListPorts;
  /** The open detail window's quest id, `null` for none. */
  readonly detailId: () => number | null;
  readonly openDetail: (request: QuestDetailRequest) => void;
  readonly closeDetail: () => void;
}

/** The ids of the four page frames (`11` .. `14` of the original). */
const PAGE_IDS = [11, 12, 13, 14] as const;
/** `GetRect` of the remove confirmation. */
export const REMOVE_BOX_SIZE = { w: 250, h: 130 } as const;

export interface QuestRegInfo {
  readonly selected: number;
  readonly open: readonly number[];
}

export class WndQuest extends WndNeuz {
  readonly appId = APP_QUEST_EX_LIST;
  readonly #deps: QuestListDeps;
  readonly #pages: QuestListPage[] = [];
  /** `m_aOpenTree`: the folders the player opened. */
  readonly #open = new Set<number>();
  /** `m_idSelQuest`. */
  #selected = -1;
  #off: (() => void) | null = null;
  #building = false;

  constructor(deps: QuestListDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.getDlgItem<TabCtrl>(WIDC_QUEST_LIST_TABCTRL);
  }
  get combo(): ComboBox | null {
    return this.getDlgItem<ComboBox>(WIDC_COMBOBOX_QUEST_LIST);
  }
  get removeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_REMOVE);
  }
  /** The tree of a tab. */
  tree(tab: number): QuestListTree | null {
    return this.#pages[tab]?.tree ?? null;
  }
  /** The tree of the selected tab. */
  get currentTree(): QuestListTree | null {
    return this.tree(this.tab?.getCurSel() ?? 0);
  }
  /** `m_aOpenTree` (tests, the probe). */
  get openFolders(): readonly number[] {
    return [...this.#open];
  }
  get selectedQuest(): number {
    return this.#selected;
  }

  // ---- OnInitialUpdate -----------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const ports = this.#deps.ports();
    const tab = this.tab;
    if (tab) {
      tab.setTabTitleAlign('center');
      for (const [i] of QUEST_TAB_KINDS.entries()) {
        const page = new QuestListPage();
        page.attach(tab, PAGE_IDS[i] ?? 11 + i);
        page.tree.setDelegate({
          checked: (node, on) => {
            this.#ticked(node, on);
          },
          rejected: () => {
            this.#deps.ports().tickRefused();
          },
        });
        page.tree.onToggle = (node) => {
          this.controlOpenTree(node);
        };
        this.#pages.push(page);
        tab.insertItem(i, {
          text: ports.tabTitle(i) || (QUEST_TAB_FALLBACK[i] ?? ''),
          page,
        });
      }
      tab.setCurSel(ports.memory().tab);
    }
    const combo = this.combo;
    if (combo) {
      combo.addWndStyle(EBS_READONLY);
      for (const tid of QUEST_GROUP_TIDS) combo.addString(this.#deps.host.text(tid));
      combo.setCurSel(ports.memory().group);
    }
    this.update();
    this.#off = ports.subscribe(() => {
      this.update();
    });
    // `Move( right - w, 110 )` and then `MoveParentCenter()` (KEEP: the second wins)
    this.moveParentCenter();
  }

  // ---- the trees --------------------------------------------------------------------------------------------------------------------

  /** `Update()`: all four trees rebuilt from the model of the group. */
  update(): void {
    if (this.#building) return;
    this.#building = true;
    try {
      const ports = this.#deps.ports();
      const model = ports.tree(ports.memory().group);
      const checked = ports.checked();
      for (const [i, page] of this.#pages.entries()) {
        const tree = page.tree;
        tree.deleteAllItems();
        const nodes = model[i] ?? [];
        const add = (parent: TreeNode | null, n: QuestTreeNodeView): void => {
          const node = tree.insertItem(parent, n.label, n.id);
          if (n.isQuest && n.completed) {
            node.color = COMPLETED_COLOR;
            tree.forbidChecking(node);
          }
          for (const child of n.children) add(node, child);
          // `OpenTreeArray`: a folder opens when its id was recorded
          if (n.children.length > 0 && this.#open.has(n.id)) tree.setOpen(node, true);
        };
        for (const n of nodes) add(null, n);
        tree.setCheckedIds(checked);
      }
      const cur = this.currentTree;
      if (cur && this.#selected >= 0) cur.setCurSel(this.#selected);
      this.refreshRemove();
    } finally {
      this.#building = false;
    }
  }

  /** `ControlOpenTree`: the open state of a folder is recorded (open) or forgotten (closed). */
  controlOpenTree(node: TreeNode): void {
    if (node.open) this.#open.add(node.data);
    else this.#open.delete(node.data);
  }

  /** A tick or untick of a leaf (the tree already toggled its box). */
  #ticked(node: TreeNode, on: boolean): void {
    const ports = this.#deps.ports();
    if (on && ports.checked().length >= 5) {
      // `m_nCheckedQuestSize >= GetMaxCheckNumber()`: the box goes back, the text appears
      this.currentTree?.setCheckedIds(ports.checked());
      ports.tickRefused();
      return;
    }
    ports.tick(node.data, on);
  }

  // ---- Remove -------------------------------------------------------------------------------------------------------------------------

  /** `RemoveQuest()`: the Remove button follows the selection (live here; the original refreshed on clicks and dialog open/close). */
  refreshRemove(): void {
    const button = this.removeButton;
    if (!button) return;
    const ports = this.#deps.ports();
    const node = this.currentTree?.getCurSel() ?? null;
    const facts = node ? ports.quest(node.data) : null;
    let enable = false;
    if (facts && (facts.active || facts.completed)) {
      if (ports.isGm()) enable = true;
      else if (facts.completed || facts.state === 14 || this.env.wm.isOpen(APP_DIALOG_EX))
        enable = false;
      else enable = !facts.noRemove;
    }
    button.enableWindow(enable);
  }

  /** The Remove button on a selected leaf. */
  #askRemove(questId: number): void {
    this.#deps.closeDetail();
    this.#deps.host.confirm(this.#deps.host.text(TID_GAME_QUEST_DELCONFIRM), () => {
      this.#deps.ports().remove(questId);
    });
  }

  // ---- notifications ---------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (
      msg !== Wnm.SELCHANGE &&
      msg !== Wnm.DBLCLK &&
      msg !== Wnm.SELCANCEL &&
      msg !== Wnm.CLICKED
    ) {
      return super.onChildNotify(msg, id, src);
    }
    const ports = this.#deps.ports();
    const tab = this.tab;
    const tabIndex = tab?.getCurSel() ?? 0;
    const node = this.tree(tabIndex)?.getCurSel() ?? null;
    let questId = 0;
    if (node) {
      this.#selected = node.data;
      if (node.children.length === 0) questId = node.data;
    }
    if (msg === Wnm.SELCHANGE) {
      if (id === WIDC_COMBOBOX_QUEST_LIST || id === WIDC_QUEST_LIST_TABCTRL) {
        this.#deps.closeDetail();
        const group = this.combo?.getCurSel() ?? ports.memory().group;
        ports.setMemory(tabIndex, group);
        this.update();
      }
    } else if (msg === Wnm.DBLCLK) {
      if (node && node.children.length > 0) this.controlOpenTree(node);
    } else if (id === WIDC_REMOVE) {
      // CLICKED / SELCANCEL
      if (node?.children.length === 0) this.#askRemove(questId);
    } else if (id === WIDC_BUTTON_CLOSE) {
      this.destroy();
    } else {
      if (node?.children.length === 0) {
        if (this.#deps.detailId() === questId) this.#deps.closeDetail();
        else {
          this.#deps.closeDetail();
          this.#deps.openDetail(ports.detail(questId));
        }
      }
      this.refreshRemove();
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `SetWndRect`: the detail window follows the list (`right - 2, top`). */
  override setWndRect(rect: Rect, onSize = false): void {
    super.setWndRect(rect, onSize);
    const detail = this.env.wm.get(APP_QUEST_EX_DETAIL);
    if (detail) detail.move({ x: rect.r - 2, y: rect.t });
  }

  // ---- registration info ----------------------------------------------------------------------------------------------------------

  override serializeRegInfo(): unknown {
    const v: QuestRegInfo = { selected: this.#selected, open: [...this.#open] };
    return v;
  }

  override restoreRegInfo(v: unknown, version: number): void {
    if (version === 0) return;
    const r = v as Partial<QuestRegInfo> | null;
    if (typeof r?.selected !== 'number' || !Array.isArray(r.open)) return;
    this.#selected = r.selected;
    this.#open.clear();
    for (const id of r.open) if (typeof id === 'number') this.#open.add(id);
    this.update();
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    // `~CWndQuest`: the detail window goes with the list
    this.#deps.closeDetail();
    super.onDestroy();
  }
}
