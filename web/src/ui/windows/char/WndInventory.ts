// `WndInventory` (`CWndInventory`, `src:_Interface/WndField.cpp:1001-3110`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9; WP-UI-21):
// `APP_INVENTORY` (240 x 464, the picture `WndInventory3D.tga`). The shell owns what the original window owned:
//   * the bag: an `ItemGrid` in a code-created page of the tab control `WIDC_INVENTORY` (one tab, `TID_GAME_ITEM`);
//   * the equipment: 19 placeholders (`WIDC_CUSTOM1..19`) painted by the `PaperDoll`, with the original's own hit tests -
//     press and drag (the shortcut carries `index = 0xffffffff`, here the part in `from`), double click = unequip, tooltips
//     of the worn item or of the empty slot, drops from the bag equip, drops on the trash slot (`WIDC_CUSTOM21`) delete;
//   * the paper doll canvas (`WIDC_CUSTOM20`, dragged with the pointer to turn it) for 41's preview;
//   * the gold (`WIDC_GOLD` picks the penya up, `WIDC_GOLD_NUM` shows it with thousands separators), the sort button
//     (`WIDC_BUTTON1`), the mask box (`WIDC_CHECK2`) and the five hide boxes created in code at the corners of the fashion slots.
// What the items mean (equip rules, the double-click dispatch, the drop table of 45 §9.4, the sends) is behind the ports.

import { ITYPE_ITEM } from '../../../gen/defs/define';
import { PARTS_MASK } from '../../../gen/defs/neuz';
import {
  APP_BANK,
  APP_INVENTORY,
  APP_REPAIR,
  APP_SHOPEX,
  APP_TRADE,
  WIDC_BUTTON1,
  WIDC_CHECK2,
  WIDC_GOLD,
  WIDC_GOLD_NUM,
  WIDC_INVENTORY,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_INVEONTORY_REMOVE_TOOL,
  TID_GAME_ITEM,
  TID_GAME_REPAIR_NOTUSE,
  TID_GAME_TRADELIMITUSING,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import { ItemGrid, type ItemGridDelegate } from '../../controls/ItemGrid';
import type { Static } from '../../controls/Static';
import type { TabCtrl } from '../../controls/TabCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import {
  type Point,
  type Rect,
  rect,
  rectContains,
  rectOffset,
  rectWidth,
} from '../../layout/geometry';
import { WBS_CHECK, WBS_NOFRAME, WSS_MONEY, WTYPE_BUTTON, Wnm } from '../../layout/styles';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CodePage } from './CodePage';
import { GoldSource } from './GoldSource';
import {
  HIDE_BOX_BASE_ID,
  HIDE_PARTS,
  buildPaperLayers,
  equipDropArea,
  partRects,
  slotPartAt,
  slotTooltip,
  wornPartAt,
} from './inventoryLayout';
import { PaperDoll, type PaperLayer } from './PaperDoll';
import type {
  CorePreviewHandle,
  CorePreviewPort,
  InventoryActions,
  InventoryItem,
  InventoryView,
  ViewSource,
} from './ports';

/** The window id of the bag grid (`m_wndItemCtrl.Create(WLVS_ICON, ..., pTabCtrl, 11)`). */
export const WIDC_BAG = 11;
/** `m_fRot` changes by half a degree per pixel of pointer travel. */
export const ROTATE_PER_PX = 0.5;
/** The trash can: 27 px, 1.2 x (32 px) under the pointer (`m_TexRemoveItem->Render(.., CPoint(27, 27), 255, 1.2, 1.2)`). */
export const TRASH_ICON = 'WndInventoryGarbage.dds';
export const TRASH_SIZE = 27;
export const TRASH_HOVER_SIZE = 32;

export interface InventoryDeps {
  readonly view: ViewSource<InventoryView>;
  readonly actions: InventoryActions;
  readonly tooltip: TooltipService;
  /** `getText(tid)` of the texts bundle. */
  readonly getText: (tid: number) => string;
  readonly previews?: CorePreviewPort | null | undefined;
}

/** The tab page that holds the bag grid; the grid follows the size the tab gives the page. */
class GridPage extends CodePage {
  grid: ItemGrid | null = null;
  readonly #delegate: ItemGridDelegate;

  constructor(delegate: ItemGridDelegate) {
    super();
    this.#delegate = delegate;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.grid = this.addControl(
      new ItemGrid(this.#delegate, true),
      WIDC_BAG,
      this.getClientRect(false),
    );
  }

  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    this.grid?.setWndRect(rect(0, 0, cx, cy));
  }
}

const EMPTY_VIEW: InventoryView = {
  bag: [],
  equip: [],
  gold: 0,
  hideCoat: [false, false, false, false, false],
  viewMask: false,
  sorting: false,
  hideFashion: false,
  newFontColor: true,
  version: 0,
  upgradeMode: false,
};

export class WndInventory extends WndNeuz {
  readonly appId = APP_INVENTORY;
  readonly #deps: InventoryDeps;
  readonly #cells = new WeakMap<InventoryItem, IconCellData>();
  #view: InventoryView = EMPTY_VIEW;
  #applied: InventoryView | null = null;
  #rects: ReadonlyMap<number, Rect> = new Map();
  #custom: ReadonlyMap<string, Rect> = new Map();
  #paper: PaperDoll | null = null;
  #grid: ItemGrid | null = null;
  #gold: Static | null = null;
  #sort: Button | null = null;
  #maskBox: Button | null = null;
  readonly #hideBoxes: Button[] = [];
  #model: Rect | null = null;
  #trash: Rect | null = null;
  #preview: CorePreviewHandle | null = null;
  /** The worn part under the last press: the drag and the unequip-by-drag send it. */
  #selectedPart: number | null = null;
  #hoverPart: number | null = null;
  #trashHover = false;
  #rotating = false;
  #oldX = 0;
  #rot = 0;

  constructor(deps: InventoryDeps) {
    super();
    this.#deps = deps;
  }

  // ---- accessors for tests and the feature layer ----------------------------------------------------------------

  get itemGrid(): ItemGrid | null {
    return this.#grid;
  }
  /**
   * The wheel anywhere over the inventory window (paper doll, gold line, tab, buttons) scrolls the bag (owner #008,
   * BFIX1). The window manager hands the wheel to the window under the pointer and bubbles it up to this frame; over
   * the grid itself (or its scroll bar) the grid handles it first.
   */
  override onMouseWheel(notches: number): void {
    this.#grid?.onMouseWheel(notches);
  }
  /** The tab control of the bag (`WIDC_INVENTORY`). */
  get tab(): TabCtrl | null {
    return this.getDlgItem<TabCtrl>(WIDC_INVENTORY);
  }
  get paperDoll(): PaperDoll | null {
    return this.#paper;
  }
  get sortButton(): Button | null {
    return this.#sort;
  }
  get maskBox(): Button | null {
    return this.#maskBox;
  }
  get hideBoxes(): readonly Button[] {
    return this.#hideBoxes;
  }
  get goldText(): Static | null {
    return this.#gold;
  }
  /** `m_fRot` (degrees). */
  get rotation(): number {
    return this.#rot;
  }
  /** The rect of the equipment placeholder of a part, in client coordinates. */
  partRect(part: number): Rect | null {
    return this.#rects.get(part) ?? null;
  }
  /** The rect of the trash slot (`WIDC_CUSTOM21`) in client coordinates. */
  get trashRect(): Rect | null {
    return this.#trash;
  }

  // ---- OnInitialUpdate -----------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const custom = new Map<string, Rect>();
    for (let i = 1; i <= 21; i += 1) {
      const name = `WIDC_CUSTOM${String(i)}`;
      const c = this.getWndCtrl(name as 'WIDC_CUSTOM1');
      if (c) custom.set(name, { ...c.rect });
    }
    this.#custom = custom;
    this.#rects = partRects((n) => custom.get(n) ?? null);
    this.#model = custom.get('WIDC_CUSTOM20') ?? null;
    this.#trash = custom.get('WIDC_CUSTOM21') ?? null;

    // the paper doll sits UNDER the controls of the layout (the mask box overlaps the mask slot): it was created last, so it goes first
    const client = this.getClientRect(false);
    const paper = new PaperDoll();
    this.addControl(paper, 0x7a01, client);
    this.controlList.splice(this.controlList.indexOf(paper), 1);
    this.controlList.unshift(paper);
    this.publish();
    this.#paper = paper;

    // the gold coin (an invisible drag source) and the amount
    const gold = this.getWndCtrl(WIDC_GOLD);
    // `CWndGold::Create( "g", WBS_NODRAWFRAME, rect, ... )`: no frame, so the whole rect of the coin takes the press (with the default frame only a 9 x 7 px
    // corner of the 21 x 21 coin was hit: found by the Playwright flow of the bank)
    if (gold) this.addControl(new GoldSource(), WIDC_GOLD, gold.rect, WBS_NOFRAME);
    this.#gold = this.getDlgItem<Static>(WIDC_GOLD_NUM);
    this.#gold?.addWndStyle(WSS_MONEY);

    // the bag: one tab "Item" whose page is the grid
    const tab = this.getDlgItem<TabCtrl>(WIDC_INVENTORY);
    if (tab) {
      const delegate: ItemGridDelegate = {
        shortcutOf: (slot) => this.#bagShortcut(slot),
        // an armed upgrade cursor completes with the pressed item; the dialogs that forbid item actions refuse the press
        press: (slot) => {
          const item = this.#view.bag[slot] ?? null;
          return item ? (this.#deps.actions.press?.(item, slot) ?? true) : true;
        },
        rightPress: () => {
          this.#deps.actions.cancelCursor?.();
        },
        dblClick: (slot) => {
          const item = this.#view.bag[slot] ?? null;
          if (item) this.#deps.actions.useItem(item, slot);
        },
        hover: itemHover({
          tooltip: this.#deps.tooltip,
          owner: this,
          flag: APP_INVENTORY,
          itemAt: (slot) => this.#view.bag[slot] ?? null,
          ctx: () => ({
            version: this.#view.version,
            equippedAt: (parts) => this.#view.equip[parts] ?? null,
          }),
        }),
        drop: (sc, slot) => this.#dropOnGrid(sc, slot),
      };
      const page = new GridPage(delegate);
      page.attach(tab, WIDC_BAG);
      this.#grid = page.grid;
      tab.insertItem(0, { text: this.#deps.getText(TID_GAME_ITEM), page });
    }

    this.#sort = this.getDlgItem<Button>(WIDC_BUTTON1);
    this.#maskBox = this.getDlgItem<Button>(WIDC_CHECK2);
    // the five hide boxes at the top-left corners of the fashion slots (`m_awndCheckHide[i].Create("", WBS_CHECK, ...)`)
    HIDE_PARTS.forEach((part, i) => {
      const r = this.#rects.get(part);
      if (!r) return;
      const box = this.createControl<Button>(
        WTYPE_BUTTON,
        HIDE_BOX_BASE_ID + i,
        rect(r.l, r.t, r.l + 16, r.t + 16),
        WBS_CHECK,
      );
      box.setVisible(false);
      this.#hideBoxes.push(box);
    });
    // the mask box sits at the top-left of the mask slot (`pWndCheck->Move(m_InvenRect[PARTS_MASK].TopLeft())`)
    const maskRect = this.#rects.get(PARTS_MASK);
    if (this.#maskBox && maskRect) this.#maskBox.move({ x: maskRect.l, y: maskRect.t });
    this.#maskBox?.setVisible(false);

    // the paper doll preview
    const canvas = this.#model ? this.customCanvas('WIDC_CUSTOM20') : null;
    if (canvas && this.#deps.previews) {
      this.#preview = this.#deps.previews.open({
        canvas,
        preset: 'inventory',
        subject: this.#deps.view.get().subject,
      });
    }

    // the window opens at the right edge of the layout rect, 160 px from the top
    const layout = this.env.wm.layoutRect;
    this.move({ x: layout.r - rectWidth(this.rectWindow), y: 112 + 48 });

    this.#applyView(this.#deps.view.get());
  }

  // ---- the view --------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#applyView(v);
    super.process(now);
  }

  #cellOf(item: InventoryItem | null): IconCellData | null {
    if (!item) return null;
    let c = this.#cells.get(item);
    if (!c) {
      c = {
        icon: item.icon,
        count: item.count,
        showCount: item.stackable,
        dim: item.extra > 0 || item.using,
        tint: item.expired ? 'red' : null,
        cooldown: item.cooldown,
      };
      this.#cells.set(item, c);
    }
    return c;
  }

  #applyView(v: InventoryView): void {
    this.#view = v;
    this.#applied = v;
    this.#grid?.setCells(v.bag.map((it) => this.#cellOf(it)));
    this.#grid?.setNewFontColor(v.newFontColor);
    this.#gold?.setTitle(String(v.gold));
    this.#sort?.setVisible(v.sorting);
    // the hide boxes follow the worn fashion pieces; the mask box the mask
    HIDE_PARTS.forEach((part, i) => {
      const box = this.#hideBoxes[i];
      if (!box) return;
      box.setVisible(v.hideFashion && (v.equip[part] ?? null) !== null);
      box.setCheck(!(v.hideCoat[i] ?? false));
    });
    this.#maskBox?.setVisible(v.hideFashion && (v.equip[PARTS_MASK] ?? null) !== null);
    this.#maskBox?.setCheck(v.viewMask);
    this.#paintPaper();
    this.#preview?.invalidate();
  }

  #paintPaper(): void {
    const v = this.#view;
    const layers: PaperLayer[] = buildPaperLayers({
      equip: v.equip,
      rects: this.#rects,
      hideCoat: v.hideCoat,
      newFontColor: v.newFontColor,
    });
    const t = this.#trash;
    if (t) {
      const size = this.#trashHover ? TRASH_HOVER_SIZE : TRASH_SIZE;
      layers.push({
        key: 'trash',
        x: t.l,
        y: t.t,
        size,
        icon: TRASH_ICON,
        alpha: 1,
        tint: false,
      });
    }
    this.#paper?.setLayers(layers, v.newFontColor);
  }

  // ---- drags from the bag and from the equipment -------------------------------------------------------------------

  /** The payload of a bag drag: kind ITEM, the object id, the slot; refused for a reserved item. */
  #bagShortcut(slot: number): Shortcut | null {
    const item = this.#view.bag[slot] ?? null;
    if (!item || item.extra > 0) return null;
    return {
      kind: SC.ITEM,
      id: item.objId,
      type: ITYPE_ITEM,
      index: slot,
      userId: 0,
      data: 1,
      itemId: item.itemId,
      ...(item.icon === null ? {} : { icon: item.icon }),
    };
  }

  /** The payload of a drag from an equipment slot (`index = 0xffffffff`: not a bag slot). */
  #equipShortcut(part: number): Shortcut | null {
    const item = this.#view.equip[part] ?? null;
    if (!item) return null;
    return {
      kind: SC.ITEM,
      id: item.objId,
      type: ITYPE_ITEM,
      index: 0xffffffff,
      userId: 0,
      data: 1,
      itemId: item.itemId,
      from: this,
      ...(item.icon === null ? {} : { icon: item.icon }),
    };
  }

  /** The APP id of the frame a shortcut came from (`pShortcut->m_pFromWnd->GetFrameWnd()->GetWndId()`), `0` if unknown. */
  #fromApp(sc: Shortcut): number {
    return sc.from?.getFrameWnd()?.id ?? 0;
  }

  /** A shortcut dropped on the bag grid (`WIN_ITEMDROP`, nID 11, WndField.cpp:1851-2199): ends with `SetForbid(bForbid)`. */
  #dropOnGrid(sc: Shortcut, slot: number): boolean {
    const ok = this.#routeGridDrop(sc, slot);
    if (!ok) this.env.wm.drag.forbid();
    return ok;
  }

  #routeGridDrop(sc: Shortcut, slot: number): boolean {
    if (sc.from === this.#grid) {
      // bag -> bag: `CWndItemCtrl::OnDropIcon`
      const item = this.#view.bag[sc.index] ?? null;
      if (!item) return false;
      this.#deps.actions.moveItem(item, sc.index, slot);
      return true;
    }
    const fromApp = this.#fromApp(sc);
    if (fromApp === APP_INVENTORY) {
      // the gold (`dwData == 0`) cannot be put into the bag; a worn item is taken off
      if (sc.data === 0) return false;
      this.#deps.actions.unequipByDrag(sc.id, this.#selectedPart ?? -1);
      return true;
    }
    return this.#deps.actions.dropIn(sc, fromApp);
  }

  /**
   * `CWndInventory::OnDropIcon` (WndField.cpp:2998-3105): a drop on the window itself (not on the grid or a button).
   * Trade, shop, bank and repair windows forbid moving inventory items around; the trash slot deletes; the equipment area
   * wears a bag item.
   */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    const from = sc.from?.getFrameWnd() ?? null;
    if (!from) return false;
    const wm = this.env.wm;
    const co = this.#clientOffset();
    const pt = { x: ptWindow.x - co.x, y: ptWindow.y - co.y };
    if (wm.isOpen(APP_SHOPEX) || wm.isOpen(APP_BANK) || wm.isOpen(APP_TRADE)) {
      if (from.id === APP_INVENTORY) {
        wm.drag.forbid();
        this.#deps.actions.message?.(TID_GAME_TRADELIMITUSING);
        return false;
      }
    } else if (wm.isOpen(APP_REPAIR)) {
      if (from.id === APP_INVENTORY) {
        wm.drag.forbid();
        this.#deps.actions.message?.(TID_GAME_REPAIR_NOTUSE);
        return false;
      }
    } else if (from.id === APP_INVENTORY) {
      const trash = this.#trash;
      // the gates of the delete route refuse silently (no forbid icon)
      if (trash && rectContains(trash, pt) && sc.type === ITYPE_ITEM && sc.data !== 0) {
        return this.#deps.actions.trash(sc);
      }
    }
    if (sc.kind === SC.ITEM && sc.from !== this && from.id === APP_INVENTORY) {
      if (sc.type === ITYPE_ITEM && sc.data !== 0) {
        const item = this.#view.bag.find((i) => i !== null && i.objId === sc.id) ?? null;
        const c5 = this.#custom.get('WIDC_CUSTOM5');
        const c10 = this.#custom.get('WIDC_CUSTOM10');
        if (item?.wearable === true && c5 && c10 && rectContains(equipDropArea(c5, c10), pt)) {
          this.#deps.actions.equip(item);
          return true;
        }
      }
    }
    // `CWndBase::OnDropIcon`
    wm.drag.forbid();
    return false;
  }

  #clientOffset(): Point {
    return {
      x: this.rectClient.l - this.rectWindow.l,
      y: this.rectClient.t - this.rectWindow.t,
    };
  }

  // ---- the pointer (OnLButtonDown / Up / DblClk / MouseMove) -----------------------------------------------------------

  override onLButtonDown(pt: Point): void {
    this.setCapture();
    const model = this.#model;
    if (model && rectContains(model, pt)) {
      this.#rotating = true;
      this.#oldX = pt.x;
    }
    const part = wornPartAt(pt, this.#rects, this.#view.equip);
    this.#selectedPart = part;
    if (part !== null && !this.env.wm.isDraggingShortcut()) {
      this.env.wm.drag.arm(this, () => this.#equipShortcut(part));
    }
  }

  override onLButtonUp(): void {
    this.#rotating = false;
    this.releaseCapture();
  }

  /** The press ended anywhere (a release outside, a pointer cancel): the paper doll stops turning. */
  override onPointerRelease(): void {
    this.#rotating = false;
  }

  override onLButtonDblClk(pt: Point): void {
    const part = wornPartAt(pt, this.#rects, this.#view.equip);
    const item = part === null ? null : (this.#view.equip[part] ?? null);
    if (part !== null && item) this.#deps.actions.unequip(item, part);
  }

  override onMouseMove(pt: Point): void {
    if (this.#rotating) {
      this.#rot += (this.#oldX - pt.x) * ROTATE_PER_PX;
      this.#oldX = pt.x;
      this.#preview?.setRotation?.(this.#rot);
    }
    if (this.env.wm.isDraggingShortcut()) return;
    this.#hover(pt);
  }

  override onMouseLeave(): void {
    this.#hoverPart = null;
    if (this.#trashHover) {
      this.#trashHover = false;
      this.#paintPaper();
    }
  }

  /** `OnMouseWndSurface` + the trash tooltip of `OnDraw`: tooltips of the slot under the pointer. */
  #hover(pt: Point): void {
    const co = this.#screenOrigin();
    const trash = this.#trash;
    const overTrash = trash !== null && rectContains(trash, pt);
    if (overTrash !== this.#trashHover) {
      this.#trashHover = overTrash;
      this.#paintPaper();
    }
    if (overTrash) {
      this.#deps.tooltip.put(
        this,
        this.#deps.getText(TID_GAME_INVEONTORY_REMOVE_TOOL),
        rectOffset(trash, co.x, co.y),
        0,
      );
      this.#hoverPart = null;
      return;
    }
    const part = slotPartAt(pt, this.#rects);
    if (part === this.#hoverPart) return;
    this.#hoverPart = part;
    if (part === null) return;
    const r = this.#rects.get(part);
    if (!r) return;
    const screen = rectOffset(r, co.x, co.y);
    const what = slotTooltip(part, this.#view.equip);
    if ('item' in what) {
      // the worn item alone: no comparison tooltips (`g_toolTip.SetSubToolTipNumber(0); PutToolTip_Item(...)`)
      this.#deps.tooltip.putItem(this, what.item, screen, APP_INVENTORY, {
        version: this.#view.version,
      });
    } else if (what.tid !== 0) {
      this.#deps.tooltip.put(this, this.#deps.getText(what.tid), screen, 0);
    }
  }

  /** The client origin in root coordinates. */
  #screenOrigin(): Point {
    const s = this.getScreenClientRect();
    return { x: s.l, y: s.t };
  }

  // ---- buttons -----------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (src === this.#sort || id === WIDC_BUTTON1) {
        this.#deps.actions.sort();
        return true;
      }
      if (src === this.#maskBox && this.#maskBox) {
        this.#deps.actions.viewMask(this.#maskBox.getCheck() !== 0);
        return true;
      }
      const i = this.#hideBoxes.findIndex((b) => b === src);
      if (i >= 0) {
        const box = this.#hideBoxes[i];
        if (box) this.#deps.actions.hideCoat(i, box.getCheck() === 0);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#preview?.close();
    this.#preview = null;
    super.onDestroy();
  }
}
