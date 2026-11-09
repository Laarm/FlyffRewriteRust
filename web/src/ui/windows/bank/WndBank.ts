// `WndBank` (`CWndBank`, `src:_Interface/WndBank.cpp:1-215`, ref/22 §17 E, Plan/42 §19.4 E, 45 §13.2; WP-UI-25): `APP_COMMON_BANK` (528 x 256, the picture
// `WndCommonBank.tga`). Three compartments side by side - one per character slot of the account - each a tab control (`WIDC_BANK_0/1/3`) with one
// tab "Item" whose page is a 42-slot item grid, the penya of the compartment as text (`WIDC_BANKGOLD_0..2`, plain digits, as `Format("%d")`) and the
// coin over it (`WIDC_BANKGO_0..2`, an invisible drag source). A compartment the player may not use (another character's without a remote-bank mode, a
// slot without a character) is greyed out by `WndCommonBankNotUse.tga` at alpha 50 over `WIDC_CUSTOM4..6` and takes no press or drop.
//
// What the shell does of `CWndBank::OnChildNotify(WIN_ITEMDROP)`: the compartment under the pointer is the target; a drop from the inventory is
// `putFromInventory(sc, slot)` (item or coin), a drop from the other compartments of this window is `moveBetween(sc, slot)`; the quantity boxes, the
// refusals and the sends are the feature's. FIX: the coin dropped on the coin icon or the penya text of a compartment goes there too (the original
// took only drops on the grids). The window opens beside the inventory, closes the pocket window and refuses to exist next to a trade, a shop, the
// guild bank or a personal shop; destroying it ends the bank session (`~CWndBank` sends `CloseBankWnd`).

import { ITYPE_ITEM } from '../../../gen/defs/define';
import {
  APP_BAG_EX,
  APP_COMMON_BANK,
  APP_GUILD_BANK,
  APP_INVENTORY,
  APP_SHOPEX,
  APP_TRADE,
  APP_VENDOREX,
  APP_VENDOR_REVISION,
} from '../../../gen/defs/resdata';
import { TID_GAME_ITEM } from '../../../gen/defs/text';
import type { ItemGridDelegate } from '../../controls/ItemGrid';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { TabCtrl } from '../../controls/TabCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { type Point, type Rect, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME } from '../../layout/styles';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { InventoryItem } from '../char/ports';
import { BankGoldIcon } from './BankGoldIcon';
import { ItemGridPage } from './ItemGridPage';
import {
  BANK_COIN_NAMES,
  BANK_GOLD_NAMES,
  BANK_NOUSE_NAMES,
  BANK_TAB_NAMES,
  LEFT_OF_INVENTORY,
  NOUSE_ALPHA,
  NOUSE_TEXTURE,
} from './names';
import { BANK_SLOTS, type BankActions, type BankView, type ViewSource } from './ports';

export interface BankDeps {
  readonly view: ViewSource<BankView>;
  readonly actions: BankActions;
  readonly tooltip: TooltipService;
  /** `getText(tid)` of the texts bundle. */
  readonly getText: (tid: number) => string;
}

export class WndBank extends WndNeuz {
  readonly appId = APP_COMMON_BANK;
  readonly #deps: BankDeps;
  readonly #cells = new WeakMap<InventoryItem, IconCellData>();
  readonly #pages: ItemGridPage[] = [];
  readonly #tabs: (TabCtrl | null)[] = [];
  readonly #golds: (Static | null)[] = [];
  readonly #coins: BankGoldIcon[] = [];
  readonly #nouse: Rect[] = [];
  #overlay: SpriteLayer | null = null;
  #applied: BankView | null = null;
  #view: BankView | null = null;
  #destroyedNotified = false;

  constructor(deps: BankDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  // ---- accessors for tests and the feature layer ------------------------------------------------------------------------

  /** The item grid of compartment `i`. */
  grid(i: number): ItemGridPage['grid'] {
    return this.#pages[i]?.grid ?? null;
  }
  /** The tab control of compartment `i`. */
  tab(i: number): TabCtrl | null {
    return this.#tabs[i] ?? null;
  }
  /** The penya text of compartment `i`. */
  goldText(i: number): Static | null {
    return this.#golds[i] ?? null;
  }
  /** The coin (drag source) of compartment `i`. */
  coin(i: number): BankGoldIcon | null {
    return this.#coins[i] ?? null;
  }
  /** The grey pictures over the unusable compartments. */
  get overlay(): SpriteLayer | null {
    return this.#overlay;
  }

  // ---- OnInitialUpdate ---------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const wm = this.env.wm;
    // `if(GetWndBase( APP_BAG_EX )) GetWndBase( APP_BAG_EX )->Destroy();`
    wm.get(APP_BAG_EX)?.destroy();
    // `if( g_WndMng.m_pWndTrade || m_pWndShop || m_pWndGuildBank || GetWndVendorBase() ) { Destroy(); return; }`
    if (
      [APP_TRADE, APP_SHOPEX, APP_GUILD_BANK, APP_VENDOREX, APP_VENDOR_REVISION].some((a) =>
        wm.isOpen(a),
      )
    ) {
      this.destroy();
      return;
    }
    for (let i = 0; i < BANK_SLOTS; i += 1) {
      const tab = this.getDlgItem<TabCtrl>(BANK_TAB_NAMES[i] ?? 'WIDC_BANK_0');
      this.#tabs.push(tab);
      this.#golds.push(this.getDlgItem<Static>(BANK_GOLD_NAMES[i] ?? 'WIDC_BANKGOLD_0'));
      const page = new ItemGridPage(this.#delegateOf(i), true);
      this.#pages.push(page);
      if (tab) {
        page.attach(tab, 11);
        tab.insertItem(0, { text: this.#deps.getText(TID_GAME_ITEM), page });
      }
      const coin = this.getWndCtrl(BANK_COIN_NAMES[i] ?? 'WIDC_BANKGO_0');
      if (coin) {
        const icon = new BankGoldIcon(i, (sc) => this.#drop(sc, i));
        // no frame (`WBS_NODRAWFRAME` of the original): the whole coin takes the press
        this.addControl(icon, coin.layout.id, coin.rect, WBS_NOFRAME);
        this.#coins.push(icon);
      }
      const noUse = this.getWndCtrl(BANK_NOUSE_NAMES[i] ?? 'WIDC_CUSTOM4');
      this.#nouse.push(noUse?.rect ?? { l: 0, t: 0, r: 0, b: 0 });
    }
    // the grey pictures sit above the grids, below nothing: a display only
    this.#overlay = this.addControl(new SpriteLayer(), 0x7b01, this.getClientRect(false));
    this.#placeNextToInventory();
    this.#apply(this.#deps.view.get());
  }

  /** `Move(ptInventory + width)` or, with the inventory on the right half of the screen, `ptInventory - width * 2.525`. */
  #placeNextToInventory(): void {
    const inv = this.env.wm.get(APP_INVENTORY);
    if (!inv) return;
    const r = inv.rectWindow;
    const w = rectWidth(r);
    const root = rectWidth(this.env.wm.viewport);
    const pt: Point =
      r.l > root / 2
        ? { x: r.l - Math.trunc(w * LEFT_OF_INVENTORY), y: r.t }
        : { x: r.l + w, y: r.t };
    this.move(pt);
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

  #apply(v: BankView): void {
    this.#view = v;
    this.#applied = v;
    const sprites: SpriteEntry[] = [];
    for (let i = 0; i < BANK_SLOTS; i += 1) {
      const comp = v.compartments[i];
      const grid = this.#pages[i]?.grid;
      if (!comp) continue;
      grid?.setNewFontColor(v.newFontColor);
      grid?.setCells(comp.items.map((it) => this.#cellOf(it)));
      // `CWndBank::ReSetBank`: an unusable compartment loses its scroll bar and its pointer
      grid?.enableWindow(comp.usable);
      this.#coins[i]?.enableWindow(comp.usable);
      if (!comp.usable) grid?.scrollBar?.setVisible(false);
      // `str.Format("%d", g_pPlayer->m_dwGoldBank[i]); pCost[i]->SetTitle(str)`
      this.#golds[i]?.setTitle(String(comp.gold));
      const r = this.#nouse[i];
      if (!comp.usable && r) {
        sprites.push({
          key: `nouse${String(i)}`,
          path: NOUSE_TEXTURE,
          x: r.l,
          y: r.t,
          w: r.r - r.l,
          h: r.b - r.t,
          alpha: NOUSE_ALPHA,
        });
      }
    }
    this.#overlay?.setEntries(sprites);
  }

  // ---- drags and drops ----------------------------------------------------------------------------------------------------------

  #delegateOf(i: number): ItemGridDelegate {
    return {
      shortcutOf: (slot) => {
        const item = this.#view?.compartments[i]?.items[slot] ?? null;
        if (!item) return null;
        const sc: Shortcut = {
          kind: SC.ITEM,
          id: item.objId,
          type: ITYPE_ITEM,
          index: slot,
          // the compartment the item leaves: the original found it through the focused child of the frame
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
        flag: APP_COMMON_BANK,
        itemAt: (slot) => this.#view?.compartments[i]?.items[slot] ?? null,
        ctx: () => ({ version: this.#view?.version ?? 0 }),
      }),
      drop: (sc) => this.#drop(sc, i),
    };
  }

  /** `WIN_ITEMDROP` with the compartment under the pointer: ends with `SetForbid(bForbid)`. */
  #drop(sc: Shortcut, slot: number): boolean {
    const ok = this.#route(sc, slot);
    if (!ok) this.env.wm.drag.forbid();
    return ok;
  }

  #route(sc: Shortcut, slot: number): boolean {
    if (sc.kind !== SC.ITEM) return false;
    const from = sc.from?.getFrameWnd() ?? null;
    if (from?.id === APP_INVENTORY) return this.#deps.actions.putFromInventory(sc, slot);
    if (from === this) {
      // a bank item or coin back onto its own compartment: nothing to move
      if (sc.userId === slot) return true;
      return this.#deps.actions.moveBetween(sc, slot);
    }
    return false;
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
