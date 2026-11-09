// `WndBagEx` (`CWndBagEx`, `src:_Interface/WndBagEx.cpp`, ref/22 §17 E, Plan/42 §19.4 E; WP-UI-25): `APP_BAG_EX`, the backpack (236 x 496, the picture
// `InventoryBag.tga`): the basic pocket on top and the two extra bags under it, each a tab control (`WIDC_BASIC`, `WIDC_EXBAG1`, `WIDC_EXBAG2`) with one
// tab "Item" whose page is an item grid. The two bags have an icon each (`WIDC_ICO_BAG1/2`): a bag that is available shows `...Bag01.bmp`, one that is not
// shows `...Bag02.bmp` and, over the grid, the grey picture `InventoryBagEff.tga` (alpha 150). The grey picture and the icons are drawn UNDER the controls
// (the original draws them in `OnDraw` of the window, the controls come after).
//
// What the shell does of the original: a drop on a grid names the target pocket (the grid under the pointer) and the source - the inventory, or another pocket
// of this window (`sc.userId`); the amount box, the refusals and the sends are the feature's (`moveIn`, `moveBetween`). An unavailable bag takes no drop. A drop on
// the window itself over a bag icon is `availBag` (a pocket item activates the bag). The icons carry a tooltip with the time that is left (none with
// `features.perm_extrabags`). The window refuses to exist next to a trade, a mail being read, a shop, the guild bank or a personal shop (`Destroy()` in
// `OnInitialUpdate`), and opens in the middle of the screen (`MoveParentCenter`).

import { ITYPE_ITEM } from '../../../gen/defs/define';
import {
  APP_BAG_EX,
  APP_GUILD_BANK,
  APP_INVENTORY,
  APP_POST_READ,
  APP_SHOPEX,
  APP_TRADE,
  APP_VENDOREX,
  APP_VENDOR_REVISION,
} from '../../../gen/defs/resdata';
import { TID_GAME_ITEM } from '../../../gen/defs/text';
import type { ItemGridDelegate } from '../../controls/ItemGrid';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TabCtrl } from '../../controls/TabCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import { ItemGridPage } from '../bank/ItemGridPage';
import type { InventoryItem } from '../char/ports';
import { toClient } from '../ultimate/slots';
import {
  BAG_ICON_ALPHA,
  BAG_ICON_EMPTY,
  BAG_ICON_NAMES,
  BAG_ICON_SIZE,
  BAG_ICON_USED,
  BAG_NOUSE_NAMES,
  NOUSE_ALPHA,
  NOUSE_SIZE,
  NOUSE_TEXTURE,
  POCKET_LAYER_ID,
  POCKET_TAB_NAMES,
} from './names';
import { POCKET_COUNT, type PocketActions, type PocketView, type ViewSource } from './ports';

export interface BagExDeps {
  readonly view: ViewSource<PocketView>;
  readonly actions: PocketActions;
  readonly tooltip: TooltipService;
  /** `getText(tid)` of the texts bundle. */
  readonly getText: (tid: number) => string;
}

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };

export class WndBagEx extends WndNeuz {
  readonly appId = APP_BAG_EX;
  readonly #deps: BagExDeps;
  readonly #cells = new WeakMap<InventoryItem, IconCellData>();
  readonly #pages: ItemGridPage[] = [];
  readonly #tabs: (TabCtrl | null)[] = [];
  /** The icon rect of bag 1 and 2 (client coordinates). */
  readonly #icons: Rect[] = [];
  /** Where the grey picture of bag 1 and 2 starts (client coordinates). */
  readonly #nouse: Rect[] = [];
  #layer: SpriteLayer | null = null;
  #applied: PocketView | null = null;
  #view: PocketView | null = null;
  #hover = -1;
  #destroyedNotified = false;
  #refused = false;

  constructor(deps: BagExDeps) {
    super();
    this.#deps = deps;
  }

  // ---- accessors for tests and the feature layer ------------------------------------------------------------------------

  /** The item grid of pocket `i`. */
  grid(i: number): ItemGridPage['grid'] {
    return this.#pages[i]?.grid ?? null;
  }
  /** The tab control of pocket `i`. */
  tab(i: number): TabCtrl | null {
    return this.#tabs[i] ?? null;
  }
  /** The sprites under the controls: the grey pictures and the bag icons. */
  get layer(): SpriteLayer | null {
    return this.#layer;
  }
  /** The sprite layer draws before (under) every other control. */
  get layerBehind(): boolean {
    return this.#layer !== null && this.controlList[0] === this.#layer;
  }
  /** The window found a trade, a shop, ... open and destroyed itself while it initialised. */
  get refused(): boolean {
    return this.#refused;
  }
  /** The icon rect of bag `i` (1 or 2) in client coordinates. */
  iconRect(i: number): Rect {
    return this.#icons[i - 1] ?? NO_RECT;
  }

  // ---- OnInitialUpdate ---------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const wm = this.env.wm;
    // `if( GetApplet( APP_TRADE ) || GetWndBase( APP_POST_READ ) || m_pWndShop || m_pWndGuildBank || GetWndVendorBase() ) { Destroy(); return; }`
    if (
      [
        APP_TRADE,
        APP_POST_READ,
        APP_SHOPEX,
        APP_GUILD_BANK,
        APP_VENDOREX,
        APP_VENDOR_REVISION,
      ].some((a) => wm.isOpen(a))
    ) {
      this.#refused = true;
      this.destroy();
      return;
    }
    for (let i = 0; i < POCKET_COUNT; i += 1) {
      const tab = this.getDlgItem<TabCtrl>(POCKET_TAB_NAMES[i] ?? 'WIDC_BASIC');
      this.#tabs.push(tab);
      const page = new ItemGridPage(this.#delegateOf(i), true);
      this.#pages.push(page);
      if (tab) {
        page.attach(tab, 11);
        tab.insertItem(0, { text: this.#deps.getText(TID_GAME_ITEM), page });
      }
    }
    for (let b = 0; b < 2; b += 1) {
      this.#icons.push(this.getWndCtrl(BAG_ICON_NAMES[b] ?? 'WIDC_ICO_BAG1')?.rect ?? NO_RECT);
      this.#nouse.push(this.getWndCtrl(BAG_NOUSE_NAMES[b] ?? 'WIDC_CUSTOM4')?.rect ?? NO_RECT);
    }
    // the pictures go under the controls of the layout (`OnDraw` of the window runs before the children)
    const layer = new SpriteLayer();
    this.addControl(layer, POCKET_LAYER_ID, this.getClientRect(false));
    this.controlList.splice(this.controlList.indexOf(layer), 1);
    this.controlList.unshift(layer);
    this.#layer = layer;
    this.#apply(this.#deps.view.get());
    // `Move( rectRoot.right - width, 110 ); MoveParentCenter();`
    this.moveParentCenter();
  }

  // ---- the view ---------------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
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

  #apply(v: PocketView): void {
    this.#view = v;
    this.#applied = v;
    const sprites: SpriteEntry[] = [];
    for (let i = 0; i < POCKET_COUNT; i += 1) {
      const pocket = v.pockets[i];
      const grid = this.#pages[i]?.grid;
      if (!pocket) continue;
      grid?.setCells(pocket.items.map((it) => this.#cellOf(it)));
      // an unavailable bag takes no press and no drop
      grid?.enableWindow(pocket.available);
      if (i === 0) continue;
      const icon = this.#icons[i - 1] ?? NO_RECT;
      if (pocket.available) {
        sprites.push(this.#iconSprite(i, BAG_ICON_USED, icon));
      } else {
        const nouse = this.#nouse[i - 1] ?? NO_RECT;
        sprites.push({
          key: `nouse${String(i)}`,
          path: NOUSE_TEXTURE,
          x: nouse.l,
          y: nouse.t,
          w: NOUSE_SIZE.w,
          h: NOUSE_SIZE.h,
          alpha: NOUSE_ALPHA,
        });
        sprites.push(this.#iconSprite(i, BAG_ICON_EMPTY, icon));
      }
    }
    this.#layer?.setEntries(sprites);
  }

  #iconSprite(i: number, path: string, icon: Rect): SpriteEntry {
    return {
      key: `icon${String(i)}`,
      path,
      x: icon.l,
      y: icon.t,
      w: BAG_ICON_SIZE,
      h: BAG_ICON_SIZE,
      alpha: BAG_ICON_ALPHA,
    };
  }

  // ---- drags and drops ----------------------------------------------------------------------------------------------------------

  #delegateOf(i: number): ItemGridDelegate {
    return {
      shortcutOf: (slot) => {
        const item = this.#view?.pockets[i]?.items[slot] ?? null;
        if (!item) return null;
        const sc: Shortcut = {
          kind: SC.ITEM,
          id: item.objId,
          type: ITYPE_ITEM,
          index: slot,
          // the pocket the item leaves: the original found it through the focused child of the frame
          userId: i,
          data: 1,
          itemId: item.itemId,
          ...(item.icon === null ? {} : { icon: item.icon }),
        };
        return sc;
      },
      hover: itemHover({
        tooltip: this.#deps.tooltip,
        owner: this,
        flag: APP_BAG_EX,
        itemAt: (slot) => this.#view?.pockets[i]?.items[slot] ?? null,
        ctx: () => ({ version: this.#view?.version ?? 0 }),
      }),
      drop: (sc) => this.#drop(sc, i),
    };
  }

  /** `WIN_ITEMDROP` (nID 11) with the grid under the pointer: an unavailable bag refuses (`if(!m_bUse[n]) return FALSE`). */
  #drop(sc: Shortcut, pocket: number): boolean {
    if (sc.kind !== SC.ITEM) return false;
    if (!(this.#view?.pockets[pocket]?.available ?? false)) return false;
    const from = sc.from?.getFrameWnd() ?? null;
    if (from?.id === APP_INVENTORY) return this.#deps.actions.moveIn(sc, pocket);
    if (from === this) {
      // a pocket item back onto its own pocket: nothing to move
      if (sc.userId === pocket) return true;
      if (!(this.#view?.pockets[sc.userId]?.available ?? false)) return false;
      return this.#deps.actions.moveBetween(sc, pocket);
    }
    return false;
  }

  /**
   * `CWndBagEx::OnDropIcon`: the bag icons take an item of the inventory. The drop point is checked against the icon of bag 1, then bag 2; a drop
   * anywhere else on the window does nothing.
   */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM) return false;
    if ((sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) return false;
    const pt = toClient(this, ptWindow);
    for (let b = 1; b <= 2; b += 1) {
      if (rectContains(this.iconRect(b), pt)) return this.#deps.actions.availBag(sc, b);
    }
    return false;
  }

  // ---- the tooltip of the bag icons ---------------------------------------------------------------------------------------------

  /** `OnMouseWndSurface`: over a bag icon, the time that is left (or the hint to put the bag item there). */
  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    let hit = -1;
    for (let b = 1; b <= 2; b += 1) {
      if (rectContains(this.iconRect(b), pt)) hit = b;
    }
    const tip = hit > 0 ? (this.#view?.pockets[hit]?.iconTip() ?? null) : null;
    if (hit < 0 || tip === null) {
      if (this.#hover >= 0) {
        this.#hover = -1;
        this.#deps.tooltip.cancel();
      }
      return;
    }
    this.#hover = hit;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.put(this, tip, rectOffset(this.iconRect(hit), s.l, s.t), 0);
  }

  override onDestroy(): void {
    this.#deps.tooltip.cancel();
    if (!this.#destroyedNotified) {
      this.#destroyedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}
