// `WndTrade` (`CWndTrade`, `src:_Interface/WndField.cpp:9308-9535` and `WndTradeCtrl.cpp`, ref/22 §17 F, Plan/42 §19.4 F, 45 §14.2; WP-UI-26):
// `APP_TRADE` (352 x 288, the single picture `WndTrade.tga`). Two 5 x 5 grids of 32 px slots, the partner's ("you", ids 10001, at (5, 40)) and
// the own one ("me", 10002, at (175, 40)); above each the name (`WIDC_STATIC1` / `WIDC_STATIC2`), under each the penya offered
// (`WIDC_STATIC3` / `WIDC_STATIC4`, thousands separated); the two OK buttons (`WIDC_OK_YOU` left, `WIDC_OK_I` right) and Cancel between them.
//
//   * An inventory item dropped on an EMPTY slot of the own grid offers it; a stack asks for the count first (the feature's quantity box). The
//     gold coin of the inventory dropped on either grid asks for the penya. The partner's grid takes no item.
//   * `WIDC_OK_I` sends OK; once a side pressed OK its button is checked and disabled (`SetCheck( TRUE ); EnableWindow( FALSE )`). Cancel and
//     the title-bar button send the cancel and the window STAYS until the answer closes it (`return TRUE`).
//   * While `APP_TRADE_CONFIRM` is open the window is deaf (`if( pWndTradeConfirm ) return( TRUE )`), grids included.
//   * `WIDC_CLEAR` (clear gold) is hidden and disabled (`__CSC_VER11_1`); the bag-pockets window closes when the trade opens, and a shop, bank,
//     guild bank or private shop window closes the trade again.
//   * It opens centred on the root. The window is not remembered in the window rect store (`SetPutRegInfo( FALSE )`).

import {
  APP_BAG_EX,
  APP_COMMON_BANK,
  APP_GUILD_BANK,
  APP_INVENTORY,
  APP_SHOPEX,
  APP_TRADE,
  APP_TRADE_CONFIRM,
  APP_VENDOR_REVISION,
  WIDC_CANCEL,
  WIDC_CLEAR,
  WIDC_OK_I,
  WIDC_OK_YOU,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { type Point, type Rect, rect } from '../../layout/geometry';
import { TRADE_SLOTS, TradeGrid } from '../../lists/TradeGrid';
import { WTBID_CLOSE, WSS_MONEY, Wnm } from '../../layout/styles';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { TradeActions, TradeCell, TradeSideView, TradeView, ViewSource } from './ports';

/** The window ids of the two grids (`m_wndItemCtrlYou.Create( .., this, 10001 )`, `m_wndItemCtrlI` 10002). */
export const WIDC_GRID_YOU = 10001;
export const WIDC_GRID_ME = 10002;
/** Their rects in the client area: `CRect( 5, 25 + 15, 5 + 32 * 5 + 5, 25 + 32 * 5 + 5 + 15 )` and the same at x = 175. */
export const GRID_YOU_RECT = rect(5, 40, 5 + 32 * 5 + 5, 25 + 32 * 5 + 5 + 15);
export const GRID_ME_RECT = rect(175, 40, 175 + 32 * 5 + 5, 25 + 32 * 5 + 5 + 15);

/** The windows that make the trade window close itself when it opens (`GetWndBase( APP_WEBBOX ) || m_pWndShop || ...`). */
export const TRADE_EXCLUDES: readonly number[] = [
  APP_SHOPEX,
  APP_COMMON_BANK,
  APP_GUILD_BANK,
  APP_VENDOR_REVISION,
];

export interface TradeDeps {
  readonly view: ViewSource<TradeView>;
  readonly actions: TradeActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

const isGold = (sc: Shortcut): boolean => sc.kind === SC.ITEM && sc.data === 0;

/** A trade grid whose gold drop is accepted on any spot of it and whose item drop only on its own side. */
class SideGrid extends TradeGrid {
  readonly #onGold: (sc: Shortcut) => void;
  readonly #own: boolean;
  readonly #onItem: (sc: Shortcut, slot: number) => void;
  readonly #onPull: (slot: number) => void;

  constructor(
    own: boolean,
    delegate: { hover: (slot: number | null, rect: Rect | null) => void },
    onGold: (sc: Shortcut) => void,
    onItem: (sc: Shortcut, slot: number) => void,
    onPull: (slot: number) => void,
  ) {
    super({ hover: delegate.hover }, true, false);
    this.#own = own;
    this.#onGold = onGold;
    this.#onItem = onItem;
    this.#onPull = onPull;
  }

  /** The right click on an item of the own offer takes it back. */
  override onRButtonUp(pt: Point): void {
    if (!this.#own) return;
    const slot = this.slotAt(pt);
    if (slot >= 0 && this.cells[slot]) this.#onPull(slot);
  }

  override onDropIcon(sc: Shortcut, pt: Point): boolean {
    // `OnDropIcon`: the penya go to the parent from anywhere on the grid, items only to a free slot of the own side
    if (isGold(sc)) {
      this.#onGold(sc);
      return true;
    }
    if (!this.#own) {
      this.env.wm.drag.forbid();
      return true;
    }
    const c = this.rectClientValue;
    const w = this.rectWindowValue;
    const slot = this.slotAt({ x: pt.x - (c.l - w.l), y: pt.y - (c.t - w.t) });
    if (slot < 0 || this.cells[slot]) return true; // the original takes the drop and does nothing
    this.#onItem(sc, slot);
    return true;
  }
}

export class WndTrade extends WndNeuz {
  readonly appId = APP_TRADE;
  readonly #deps: TradeDeps;
  readonly #cells = new WeakMap<TradeCell, IconCellData>();
  #you: SideGrid | null = null;
  #me: SideGrid | null = null;
  #applied: TradeView | null = null;
  #off: (() => void) | null = null;
  #closedNotified = false;
  #texts: Record<'youName' | 'meName' | 'youGold' | 'meGold', Static | null> = {
    youName: null,
    meName: null,
    youGold: null,
    meGold: null,
  };

  constructor(deps: TradeDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  // ---- accessors for tests and the feature layer -----------------------------------------------------------------------

  get youGrid(): TradeGrid | null {
    return this.#you;
  }
  get meGrid(): TradeGrid | null {
    return this.#me;
  }
  get okMe(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK_I);
  }
  get okYou(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK_YOU);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }
  get clearButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CLEAR);
  }
  get names(): { you: string; me: string } {
    return {
      you: this.#texts.youName?.getTitle() ?? '',
      me: this.#texts.meName?.getTitle() ?? '',
    };
  }
  get golds(): { you: string; me: string } {
    return {
      you: this.#texts.youGold?.getTitle() ?? '',
      me: this.#texts.meGold?.getTitle() ?? '',
    };
  }

  // ---- OnInitialUpdate -------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const wm = this.env.wm;
    // the layout's statics: the names and the penya (`OnDraw` rewrites them every frame)
    this.#texts = {
      youName: this.getDlgItem<Static>(WIDC_STATIC1),
      meName: this.getDlgItem<Static>(WIDC_STATIC2),
      youGold: this.getDlgItem<Static>(WIDC_STATIC3),
      meGold: this.getDlgItem<Static>(WIDC_STATIC4),
    };
    this.#texts.youGold?.addWndStyle(WSS_MONEY);
    this.#texts.meGold?.addWndStyle(WSS_MONEY);

    this.#you = this.addControl(
      new SideGrid(
        false,
        { hover: this.#hover('you') },
        (sc) => {
          this.#drop(sc, null, true);
        },
        () => undefined,
        () => undefined,
      ),
      WIDC_GRID_YOU,
      GRID_YOU_RECT,
    );
    this.#me = this.addControl(
      new SideGrid(
        true,
        { hover: this.#hover('me') },
        (sc) => {
          this.#drop(sc, null, false);
        },
        (sc, slot) => {
          this.#drop(sc, slot, false);
        },
        (slot) => {
          if (!this.locked) this.#deps.actions.pull(slot);
        },
      ),
      WIDC_GRID_ME,
      GRID_ME_RECT,
    );

    // `__CSC_VER11_1`: the clear-gold button is neither enabled nor visible
    const clear = this.clearButton;
    clear?.enableWindow(false);
    clear?.setVisible(false);

    this.moveParentCenter();

    // the pockets window goes; a shop, bank, guild bank or private shop window ends the trade before it started
    wm.get(APP_BAG_EX)?.destroy();
    if (wm.anyOpen(TRADE_EXCLUDES) || !this.#deps.view.get().active) {
      this.destroy();
      return;
    }
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  // ---- the view --------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #cellOf(cell: TradeCell): IconCellData {
    let c = this.#cells.get(cell);
    if (!c) {
      c = {
        icon: cell.icon,
        count: cell.count,
        showCount: cell.showCount,
        tint: cell.expired ? 'red' : null,
      };
      this.#cells.set(cell, c);
    }
    return c;
  }

  #apply(v: TradeView): void {
    this.#applied = v;
    this.#side(v.you, this.#you, this.#texts.youName, this.#texts.youGold, this.okYou);
    this.#side(v.me, this.#me, this.#texts.meName, this.#texts.meGold, this.okMe);
  }

  #side(
    side: TradeSideView,
    grid: TradeGrid | null,
    name: Static | null,
    gold: Static | null,
    ok: Button | null,
  ): void {
    if (name && name.getTitle() !== side.name) name.setTitle(side.name);
    const g = String(side.gold);
    if (gold && gold.getTitle() !== g) gold.setTitle(g);
    const cells: (IconCellData | null)[] = [];
    for (let i = 0; i < TRADE_SLOTS; i += 1) {
      const c = side.cells[i];
      cells.push(c ? this.#cellOf(c) : null);
    }
    grid?.setCells(cells);
    if (ok) {
      // `SetCheck( TRUE ); EnableWindow( FALSE )` once the side pressed OK
      if ((ok.getCheck() === 1) !== side.ok) ok.setCheck(side.ok);
      if (ok.isWindowEnabled() === side.ok) ok.enableWindow(!side.ok);
    }
  }

  #hover(side: 'you' | 'me'): (slot: number | null, r: Rect | null) => void {
    return itemHover({
      tooltip: this.#deps.tooltip,
      owner: this,
      flag: APP_INVENTORY,
      itemAt: (slot) => {
        const v = this.#deps.view.get();
        return (side === 'you' ? v.you : v.me).cells[slot] ?? null;
      },
    });
  }

  // ---- drops -----------------------------------------------------------------------------------------------------------------

  /** The final confirmation is open: `GetWndBase( APP_TRADE_CONFIRM )` makes the whole window deaf. */
  get locked(): boolean {
    return this.env.wm.isOpen(APP_TRADE_CONFIRM);
  }

  #drop(sc: Shortcut, slot: number | null, partnerGrid: boolean): void {
    if (this.locked) return;
    if (!this.#deps.actions.drop(sc, slot, partnerGrid)) this.env.wm.drag.forbid();
  }

  /** A drop on the frame itself is refused (the originals' `OnDropIcon` of the frame forbids). */
  override onDropIcon(sc: Shortcut): boolean {
    if (isGold(sc)) {
      this.#drop(sc, null, false);
      return true;
    }
    return false;
  }

  // ---- notifications ---------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (this.locked) return true; // the final confirmation is open: nothing reaches the window
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK_I) {
        this.#deps.actions.ok();
        return true;
      }
      if (id === WIDC_CANCEL || id === WTBID_CLOSE) {
        this.#deps.actions.cancel();
        return true; // the window stays until the server's answer closes it
      }
      if (id === WIDC_CLEAR) return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#deps.tooltip.cancel();
    if (!this.#closedNotified) {
      this.#closedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}
