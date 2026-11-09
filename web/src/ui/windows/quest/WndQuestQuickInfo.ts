// `WndQuestQuickInfo` (`CWndQuestQuickInfo`, `src:_Interface/WndQuestQuickInfo.cpp:1-376`, ref/22 §17 G, Plan/42 §19.4 G, 45 §28.1, §7.1; WP-UI-53): the quest tracker `APP_QUEST_QUICK_INFO` (857,
// 224 x 144, `WBS_MANAGER`, a resizable thick frame, no close button, not an applet, no registration info). A tree over the whole client: one category per ticked quest (its title) with the condition
// lines below it. Created at field entry (or as soon as the gate opens while in the world), at `right - width, 220`, visible while a quest is ticked.
//
//   * the content is rebuilt when the ticked count changes or the request says so (`SetUpdateTextSwitch`); a node opens or closes with its remembered state (default open, `CTreeInformation`);
//     a growing count and opening a node grow the window to the content, never shrink it (`ExtendAppletHeight`: items height + frame margin + 4); the size is clamped to 128..768 x 96..544;
//   * the hover fade (`Process`): `alphaCount` moves +-500/s between 0 and the window alpha while the pointer is over the window (or it is dragged); the frame and the title are drawn at
//     `alphaCount`, the text in the outlined world font at 0 and in the text font above, the colours blend (category `0xffffff00` -> `0xffa11c46`, normal `0xffffffff` -> `0xff7950ff`, selected
//     `0xffff0000` both) by `makeTextColor( .., alphaCount, alpha )`;
//   * a press on a title toggles the detail window and clears the destination, on a line runs the goal path (both through the request); the transparent frame still takes the press (KEEP: the
//     original's `m_bActiveGroundClick` is never set).

import { APP_QUEST_QUICK_INFO, WIDC_TREECTRL_QUICK_INFO } from '../../../gen/defs/resdata';
import type { Rect } from '../../layout/geometry';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_MANAGER, WBS_VSCROLL } from '../../layout/styles';
import { normalizeUiOptions } from '../../options';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { QuestQuickInfoRequest } from './ports';
import { QuickInfoTree } from './QuickInfoTree';
import { makeTextColor } from './rows';

/** The fade speed in alpha units per second (`ANIMATION_SPEED`). */
export const QUICK_FADE_SPEED = 500;
/** The window starts at `right - width, 220` (`TOP_MARGIN`). */
export const QUICK_TOP_MARGIN = 220;
/** The size limits of the window (`16 px` tiles: 8 x 6 .. 48 x 34). */
export const QUICK_MIN = { w: 128, h: 96 } as const;
export const QUICK_MAX = { w: 768, h: 544 } as const;
/** `ExtendAppletHeight` adds this to the content. */
export const QUICK_EXTEND_PAD = 4;

/** The colours of the text without and with the pointer (`D3DCOLOR_ARGB( 255, ... )`). */
export const QUICK_COLORS = {
  categoryAway: 0xffffff00,
  categoryOver: 0xffa11c46,
  normalAway: 0xffffffff,
  normalOver: 0xff7950ff,
  selected: 0xffff0000,
} as const;

export class WndQuestQuickInfo extends WndNeuz {
  readonly appId = APP_QUEST_QUICK_INFO;
  override readonly processEveryFrame = true;
  readonly #request: QuestQuickInfoRequest;
  #tree: QuickInfoTree | null = null;
  #off: (() => void) | null = null;
  /** `m_nAlphaCount`. */
  #alphaCount = 0;
  #tmOld = -1;
  /** `m_nTreeInformationListSize`. */
  #listed = 0;
  /** `m_bUpdateText`. */
  #update = true;

  constructor(request: QuestQuickInfoRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
    this.noCloseButton = true;
  }

  /** `WBS_MANAGER`: the window sits below every other window (`InitDialog( ..., WBS_MANAGER, ... )`). */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MANAGER | extraStyle);
  }

  get tree(): QuickInfoTree | null {
    return this.#tree;
  }
  /** `m_nAlphaCount` (tests, the probe). */
  get alphaCount(): number {
    return this.#alphaCount;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const at = this.getWndCtrl(WIDC_TREECTRL_QUICK_INFO);
    if (at) {
      const tree = new QuickInfoTree();
      tree.onToggle = (node) => {
        this.#request.setOpen(node.data, node.open);
        if (node.open) this.extendHeight();
      };
      tree.onPress = (press) => {
        if (press.kind === 'title') this.#request.clickTitle(press.questId);
        else if (press.line) this.#request.clickLine(press.questId, press.line);
      };
      this.addControl(tree, at.layout.id, at.rect, at.layout.style | WBS_VSCROLL);
      this.#tree = tree;
    }
    // `Move( rectRoot.right - width, TOP_MARGIN )`
    const root = this.env.wm.viewport;
    const w = rectWidth(this.rectWindow);
    this.move({ x: root.r - w, y: QUICK_TOP_MARGIN });
    this.#off = this.#request.subscribe(() => {
      // a hidden frame is not processed: it rebuilds (and shows itself) right away
      this.#update = true;
      if (!this.isVisible()) {
        this.#update = false;
        this.rebuild();
      }
    });
    this.#fitTree();
    this.rebuild();
  }

  /** `OnSize`: the tree takes the whole client. */
  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    this.#fitTree();
  }

  #fitTree(): void {
    if (!this.#tree?.isCreated) return;
    const c = this.rectClient;
    this.#tree.setWndRect({ l: 0, t: 0, r: rectWidth(c), b: rectHeight(c) }, true);
  }

  /** `SetWndRect`: the size stays within 128 x 96 .. 768 x 544. */
  override setWndRect(rect: Rect, onSize = false): void {
    const w = Math.min(QUICK_MAX.w, Math.max(QUICK_MIN.w, rectWidth(rect)));
    const h = Math.min(QUICK_MAX.h, Math.max(QUICK_MIN.h, rectHeight(rect)));
    super.setWndRect({ l: rect.l, t: rect.t, r: rect.l + w, b: rect.t + h }, onSize);
  }

  /** `ExtendAppletHeight`: grows to the content, never shrinks. */
  extendHeight(): void {
    const tree = this.#tree;
    if (!tree) return;
    const items = tree.itemsHeight();
    const win = this.rectWindow;
    const treeH = rectHeight(tree.rectWindow);
    const margin = rectHeight(win) - treeH;
    if (items > treeH) this.setWndSize(rectWidth(win), items + margin + QUICK_EXTEND_PAD);
  }

  /** The tree rebuilt from the request (the ticked quests and their lines). */
  rebuild(): void {
    const tree = this.#tree;
    if (!tree) return;
    const quests = this.#request.view();
    tree.setQuests(quests);
    this.#recolor();
    const count = this.#request.checkedCount();
    if (count > 0) this.setVisible(true);
    else this.setVisible(false);
    if (count > this.#listed) this.extendHeight();
    this.#listed = count;
  }

  /** `m_nAlpha`: the window alpha of the options. */
  #windowAlpha(): number {
    const o = this.env.host.options;
    return o ? normalizeUiOptions(o.get()).windowAlpha : 255;
  }

  #recolor(): void {
    const tree = this.#tree;
    if (!tree) return;
    const a = this.#alphaCount;
    const max = this.#windowAlpha();
    const c = QUICK_COLORS;
    tree.setTextColor(
      makeTextColor(c.categoryAway, c.categoryOver, a, max),
      makeTextColor(c.normalAway, c.normalOver, a, max),
      makeTextColor(c.selected, c.selected, a, max),
      makeTextColor(c.selected, c.selected, a, max),
    );
    tree.setWorldFont(a === 0);
  }

  /** `m_bOnSurface`: the pointer is over the window, or it is being moved or resized. */
  #onSurface(): boolean {
    const wm = this.env.wm;
    const p = wm.lastPointer;
    const r = this.rectWindow;
    const inside = p !== null && p.x >= r.l && p.x < r.r && p.y >= r.t && p.y < r.b;
    return inside || (wm.pointerPressed && wm.hover?.getFrameWnd() === this);
  }

  /** `Process`: the hover fade, then the content when the ticked count or the request changed. */
  override process(now: number): void {
    super.process(now);
    const dt = this.#tmOld < 0 ? 0 : (now - this.#tmOld) / 1000;
    this.#tmOld = now;
    const step = Math.trunc(QUICK_FADE_SPEED * dt);
    const max = this.#windowAlpha();
    if (this.#onSurface()) this.#alphaCount = Math.min(max, this.#alphaCount + step);
    else this.#alphaCount = Math.max(0, this.#alphaCount - step);
    this.setSkinAlpha(this.#alphaCount);
    this.#recolor();
    const count = this.#request.checkedCount();
    if (count !== this.#listed || this.#update) {
      this.#update = false;
      this.rebuild();
    }
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#tree?.deleteAllItems();
    super.onDestroy();
  }
}
