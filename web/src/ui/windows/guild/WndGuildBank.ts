// `WndGuildBank` (`CWndGuildBank`, `src:_Interface/WndGuildBank.cpp:60-290`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-47, WP-CFEAT-38): `APP_GUILD_BANK` (204 x 348, the picture
// `WndGuildBank.tga`), the guild's storage: a tab control (`WIDC_TABCTRL1`) with one tab "Item" holding a 42-slot item grid, the guild penya (`WIDC_GUILDBANK_NUM`, money style), the
// coin over `WIDC_CUSTOM1` (a drag source: the penya goes to the inventory, `BankGoldIcon`), and the buttons support (`WIDC_BUTTON1`: the contribution window), cloak (`WIDC_BUTTON2`, master
// only) and log (`WIDC_LOG`). `WIDC_BUTTON3` (the dragon cloak of the old siege) stays hidden. The window modelled on `WndBank`: it opens beside the inventory, closes the pocket
// window, refuses to exist next to a trade, a shop, the personal bank or a personal shop (`Destroy()` in `OnInitialUpdate`), and destroying it (the close button, Esc, death, a world
// change, a transactional close) ends the session through `actions.closed()` exactly once.
//
// What the shell does of `OnChildNotify(WIN_ITEMDROP)`: an item dragged from the INVENTORY onto the grid is `putFromInventory(sc)`; the coin dropped on the window is refused
// (contributions are the way to give penya). Taking things out is the inventory's: a bank item or the coin dragged onto the inventory.

import {
  APP_BAG_EX,
  APP_COMMON_BANK,
  APP_GUILD_BANK,
  APP_INVENTORY,
  APP_SHOPEX,
  APP_TRADE,
  APP_VENDOREX,
  APP_VENDOR_REVISION,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_CUSTOM1,
  WIDC_GUILDBANK_NUM,
  WIDC_LOG,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import { ITYPE_ITEM } from '../../../gen/defs/define';
import { TID_GAME_ITEM } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { ItemGridDelegate } from '../../controls/ItemGrid';
import type { Static } from '../../controls/Static';
import type { TabCtrl } from '../../controls/TabCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { type Point, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME, WSS_MONEY, Wnm } from '../../layout/styles';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { BankGoldIcon } from '../bank/BankGoldIcon';
import { ItemGridPage } from '../bank/ItemGridPage';
import { LEFT_OF_INVENTORY } from '../bank/names';
import type { InventoryItem } from '../char/ports';
import type { GuildBankActions, GuildBankView, ViewSource } from './ports';

export interface GuildBankDeps {
  readonly view: ViewSource<GuildBankView>;
  readonly actions: GuildBankActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

export class WndGuildBank extends WndNeuz {
  readonly appId = APP_GUILD_BANK;
  readonly #deps: GuildBankDeps;
  readonly #cells = new WeakMap<InventoryItem, IconCellData>();
  #page: ItemGridPage | null = null;
  #tab: TabCtrl | null = null;
  #gold: Static | null = null;
  #coin: BankGoldIcon | null = null;
  #view: GuildBankView | null = null;
  #applied: GuildBankView | null = null;
  #closedNotified = false;

  constructor(deps: GuildBankDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  // ---- accessors for tests and the feature layer ------------------------------------------------------------------------

  get grid(): ItemGridPage['grid'] {
    return this.#page?.grid ?? null;
  }
  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get goldText(): Static | null {
    return this.#gold;
  }
  get coin(): BankGoldIcon | null {
    return this.#coin;
  }
  get supportButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get cloakButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get dragonButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON3);
  }
  get logButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_LOG);
  }

  // ---- OnInitialUpdate ---------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const wm = this.env.wm;
    // `if(GetWndBase( APP_BAG_EX )) GetWndBase( APP_BAG_EX )->Destroy();`
    wm.get(APP_BAG_EX)?.destroy();
    // `if( m_pWndTrade || m_pWndShop || m_pWndBank || GetWndVendorBase() ) { Destroy(); return; }`
    if (
      [APP_TRADE, APP_SHOPEX, APP_COMMON_BANK, APP_VENDOREX, APP_VENDOR_REVISION].some((a) =>
        wm.isOpen(a),
      )
    ) {
      this.destroy();
      return;
    }
    this.#gold = this.getDlgItem<Static>(WIDC_GUILDBANK_NUM);
    this.#gold?.addWndStyle(WSS_MONEY);
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    this.#tab = tab;
    const page = new ItemGridPage(this.#delegate(), true);
    this.#page = page;
    if (tab) {
      page.attach(tab, 11);
      tab.insertItem(0, { text: this.#deps.getText(TID_GAME_ITEM), page });
    }
    const coin = this.getWndCtrl(WIDC_CUSTOM1);
    if (coin) {
      const icon = new BankGoldIcon(0, (sc) => this.#dropOnCoin(sc));
      // no frame (`WBS_NODRAWFRAME` of the original): the whole coin takes the press
      this.addControl(icon, coin.layout.id, coin.rect, WBS_NOFRAME);
      this.#coin = icon;
    }
    // `m_wndGold` ... the dead dragon-cloak button of the old siege
    const dragon = this.dragonButton;
    if (dragon) {
      dragon.enableWindow(false);
      dragon.setVisible(false);
    }
    this.logButton?.enableWindow(true);
    this.logButton?.setVisible(true);
    this.#placeNextToInventory();
    this.#apply(this.#deps.view.get());
  }

  /** `ptInventory.x > width / 2 ? ptInventory - (width, 0) : ptInventory + (inventory width, 0)`. */
  #placeNextToInventory(): void {
    const inv = this.env.wm.get(APP_INVENTORY);
    if (!inv) return;
    const r = inv.rectWindow;
    const iw = rectWidth(r);
    const w = rectWidth(this.rectWindow);
    const root = rectWidth(this.env.wm.viewport);
    const pt: Point =
      r.l > root / 2
        ? { x: r.l - Math.trunc(w * LEFT_OF_INVENTORY), y: r.t }
        : { x: r.l + iw, y: r.t };
    this.move(pt);
  }

  // ---- the view ------------------------------------------------------------------------------------------------------------------

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

  #apply(v: GuildBankView): void {
    this.#view = v;
    this.#applied = v;
    const grid = this.#page?.grid;
    grid?.setNewFontColor(v.newFontColor);
    grid?.setCells(v.items.map((it) => this.#cellOf(it)));
    // `m_nGoldGuild` as a money text
    this.#gold?.setTitle(String(v.gold));
    // the cloak button exists for the master only
    this.cloakButton?.enableWindow(v.isMaster);
  }

  // ---- drags and drops ----------------------------------------------------------------------------------------------------------

  #delegate(): ItemGridDelegate {
    return {
      shortcutOf: (slot) => {
        const item = this.#view?.items[slot] ?? null;
        if (!item) return null;
        const sc: Shortcut = {
          kind: SC.ITEM,
          // `m_dwId` = the bank object id: what `GetItemGuildBank` names
          id: item.objId,
          type: ITYPE_ITEM,
          index: slot,
          userId: 0,
          data: 1,
          itemId: item.itemId,
          ...(item.icon === null ? {} : { icon: item.icon }),
        };
        return sc;
      },
      hover: itemHover({
        tooltip: this.#deps.tooltip,
        owner: this,
        flag: APP_GUILD_BANK,
        itemAt: (slot) => this.#view?.items[slot] ?? null,
      }),
      drop: (sc) => this.#drop(sc),
    };
  }

  /** `WIN_ITEMDROP` (`nID == 11`): only the inventory puts things in; ends with `SetForbid(bForbid)`. */
  #drop(sc: Shortcut): boolean {
    const ok = this.#route(sc);
    if (!ok) this.env.wm.drag.forbid();
    return ok;
  }

  #route(sc: Shortcut): boolean {
    // the penya coin (`data == 0`) is never deposited
    if (sc.kind !== SC.ITEM || sc.data === 0) return false;
    const from = sc.from?.getFrameWnd() ?? null;
    if (from?.id !== APP_INVENTORY) return false;
    return this.#deps.actions.putFromInventory(sc);
  }

  /** The coin icon takes a dropped inventory ITEM like the grid; the penya coin itself is refused (`data == 0`). */
  #dropOnCoin(sc: Shortcut): boolean {
    if (sc.data === 0) {
      this.env.wm.drag.forbid();
      return false;
    }
    return this.#drop(sc);
  }

  // ---- buttons ----------------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const a = this.#deps.actions;
      switch (id) {
        case WIDC_BUTTON1:
          a.support();
          return true;
        case WIDC_BUTTON2:
          a.cloak();
          return true;
        case WIDC_LOG:
          a.log();
          return true;
        default:
          break;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#deps.tooltip.cancel();
    if (!this.#closedNotified) {
      this.#closedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}
