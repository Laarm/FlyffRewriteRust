// The `npc` feature, shop part (Plan/45 §12, Plan/19 §7; WP-CFEAT-19): the NPC shop window and everything around it. The server
// answers `OpenShopWndReq` with `OpenShopWndNtf` (the four stock tabs, header = the vendor). A stock item is bought by dragging
// it to the bag grid or by a double click (`APP_CONFIRM_BUYEX` asks the quantity); an inventory item is sold by dropping it on
// the shop (`APP_WARNING` first for a premium item, then `APP_CONFIRM_SELL`); the Quick Sell button opens `APP_QUICK_SELL`.
// Every price is the wasm rules' (`prices.ts`). The shop window ends the session: closing it sends `CloseShopWnd`.
//
// Other features reach it with `shopOf(ctx)`.

import type { ClientContext } from '../../../app/context';
import { EntityKind } from '../../../game/entities';
import {
  APP_CONFIRM_BUYEX,
  APP_CONFIRM_SELL,
  APP_QUICK_SELL,
  APP_SHOPEX,
  APP_WARNING,
} from '../../../gen/defs/resdata';
import { II_CHP_RED, II_SYS_SYS_SCR_PERIN } from '../../../gen/defs/item';
import {
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_CLOTH,
  IK2_CLOTHETC,
  IK2_RIDING,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  IK3_ARROW,
  IK3_BCHARM,
  IK3_RCHARM,
  WEAPON_UNIQUE,
} from '../../../gen/defs/itemkind';
import { PARTS_RWEAPON } from '../../../gen/defs/neuz';
import { MMI_TRADE } from '../../../gen/defs/neuz';
import {
  TID_DIAG_0006,
  TID_DIAG_0009,
  TID_DIAG_0048,
  TID_DIAG_0086,
  TID_DIAG_0087,
  TID_GAME_CANNT_BY_REDCHIP,
  TID_GAME_CANNTBUY_REDCHIP,
  TID_GAME_EQUIPTRADE,
  TID_GMAE_CHAOTIC_NOT_SHOP,
} from '../../../gen/defs/text';
import type { SpawnNpc } from '../../../gen/proto/flyff/v1/world_pb';
import { rulesRuntime } from '../../../game/rules';
import { SC, type Shortcut } from '../../../ui/dnd/shortcut';
import type {
  ConfirmBuyRequest,
  ConfirmSellRequest,
  QuickSellActions,
  QuickSellView,
  ShopActions,
  ShopCell,
  ShopPick,
  ShopTabView,
  ShopView,
  ViewSource,
} from '../../../ui/windows/npc/ports';
import { type FeatureContext, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { itemIconPath } from '../../common/itemIcon';
import { sysChat } from '../../common/text';
import { derivedView } from '../../common/view';
import { taxOf } from '../../events/tax/taxModule';
import { tooltipOf } from '../../tooltip';
import { isCharged, isExpired, isQuestItem, NULL_ID, partsOf } from '../../inventory/info';
import type { InvItem } from '../../inventory/items';
import { inventoryOf } from '../../inventory/window/inventoryModule';
import type { MenuFeature } from '../menuModule';
import { busyText } from '../busy';
import { shopPricesOf, type ShopPrices } from './prices';
import { quickSellCandidates, quickSellTotal } from './quickSell';
import { createShopSenders, type ShopSenders } from './senders';
import {
  applyOpenShop,
  applyUpdateVendor,
  applyVendor,
  closedShopState,
  defineShopStore,
  type ShopStockEntry,
  type ShopState,
  type ShopStore,
} from './state';

/** `MAX_BUY_ITEMCOUNT`: the most one purchase takes, 9999 for arrows, charms and Red Chips, 21 for the Perin. */
export const MAX_BUY_ITEMCOUNT = 99;
export const MAX_BUY_BULK = 9999;
export const MAX_BUY_PERIN = 21;

/** The ceiling of the quantity box for an item (`CWndConfirmBuy`). */
export function maxBuyQuantity(prop: { item_kind3: number } | undefined, itemId: number): number {
  if (itemId === II_SYS_SYS_SCR_PERIN) return MAX_BUY_PERIN;
  if (
    itemId === II_CHP_RED ||
    prop?.item_kind3 === IK3_BCHARM ||
    prop?.item_kind3 === IK3_RCHARM ||
    prop?.item_kind3 === IK3_ARROW
  ) {
    return MAX_BUY_BULK;
  }
  return MAX_BUY_ITEMCOUNT;
}

const USABLE_KINDS: ReadonlySet<number> = new Set([
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_CLOTH,
  IK2_CLOTHETC,
]);

/** The public face of the shop feature of a context. */
export interface ShopFeature {
  readonly store: ShopStore;
  readonly view: ViewSource<ShopView>;
  readonly actions: ShopActions;
  readonly senders: ShopSenders;
  readonly prices: ShopPrices;
  /** `MMI_TRADE` on an NPC: the busy checks, then `OpenShopWndReq`. */
  requestOpen(npcObjId: number): boolean;
  /** Closes the shop window (`CloseShopWnd` follows from its destruction). */
  close(): void;
  isOpen(): boolean;
  state(): ShopState;
  /** The quick sale window is open. */
  quickSellOpen(): boolean;
  /** Opens the quick sale window (the Quick Sell button). */
  openQuickSell(): boolean;
}

const features = new WeakMap<ClientContext, ShopFeature>();

/** The shop feature of a context; `undefined` before the module registered. */
export function shopOf(ctx: ClientContext): ShopFeature | undefined {
  return features.get(ctx);
}

export function registerShop(fc: FeatureContext, menu: MenuFeature): ShopFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const store = defineShopStore(ctx.stores.registry);
  const senders = createShopSenders(ctx);
  const prop = (id: number) => ctx.data?.item(id);
  const prices = shopPricesOf(ctx, (id) => prop(id)?.cost ?? 0);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
  };
  const stats = { opened: 0, bought: 0, sold: 0, quickSold: 0 };
  const inv = () => inventoryOf(ctx);

  // while the window is open the item tooltips carry its prices: a buy price on the shop grid, a sell price on the inventory
  // (`tooltipOf(ctx).setShop`, Plan/45 §10; the seam was left to this feature by the tooltip module and not called before the M4 demo)
  const setTooltipShop = (): void => {
    const st = store.getState();
    const tip = tooltipOf(ctx);
    if (!tip) return;
    if (!st.open) {
      tip.setShop(null);
      return;
    }
    const row = st.npcKey === '' ? undefined : ctx.data?.npc(st.npcKey);
    // the rates and event factors are the server's runtime values (1.0 here); the tax is the one of the player's area (WP-CFEAT-52, `events/tax`)
    const tax = taxOf(ctx);
    const applies = tax?.applies() === true;
    tip.setShop({
      context: { vendorType: row?.vendor_type === 1 ? 1 : 0, taxApplies: applies },
      rules: applies ? { purchaseTaxRate: tax.purchaseRate(), salesTaxRate: tax.salesRate() } : {},
    });
  };
  store.subscribe((st, prev) => {
    if (st.open === prev.open && st.npcKey === prev.npcKey) return;
    setTooltipShop();
  });
  // the tax slice or the area of the player changes while the shop is open: the tooltips follow (the grid prices follow through the view key below)
  let taxOff: (() => void) | null = null;
  store.subscribe((st, prev) => {
    if (st.open === prev.open) return;
    taxOff?.();
    taxOff = st.open
      ? (taxOf(ctx)?.subscribe(() => {
          setTooltipShop();
        }) ?? null)
      : null;
  });

  // ---- the view ------------------------------------------------------------------------------------------------------------------

  const cellCache = new WeakMap<ShopStockEntry, { key: string; cell: ShopCell }>();
  // the state OBJECT is part of the identity: every open, restock and stock change makes a new one, so a later shop can
  // never hit the view of an earlier one, whatever the version counter says (bug #012; like `dialogModule`)
  let cached: { st: ShopState; key: string; view: ShopView } | null = null;

  const unusable = (it: InvItem, rweapon: number): boolean => {
    const p = prop(it.itemId);
    if (!p) return false;
    const s = selfStore.getState();
    if (isExpired(it)) return true;
    if (USABLE_KINDS.has(p.item_kind2)) {
      const rules = rulesRuntime(ctx).rules();
      if (rules) return rules.equipCheck(it.itemId, 0, s.sex, s.job, s.level, rweapon) !== 0;
      // without the rules: the sex and the level requirement of the prop
      if (p.item_sex !== NULL_ID && p.item_sex !== s.sex) return true;
      return p.limit_level1 !== NULL_ID && s.level < p.limit_level1;
    }
    if (p.item_kind2 === IK2_RIDING) return s.flightLevel < p.flight_limit;
    return false;
  };

  const build = (): ShopView => {
    const st = store.getState();
    const s = selfStore.getState();
    const row = st.npcKey === '' ? undefined : ctx.data?.npc(st.npcKey);
    const vendorType = row?.vendor_type === 1 ? 1 : 0;
    const chips = inv()?.countOf(II_CHP_RED) ?? 0;
    const rweapon = inv()?.equipped(PARTS_RWEAPON)?.itemId ?? 0;
    const rulesReady = rulesRuntime(ctx).rules() !== null;
    const key = [
      st.version,
      st.npcId,
      st.npcKey,
      ctx.data?.version ?? -1,
      s.sex,
      s.level,
      s.job,
      s.flightLevel,
      rweapon,
      vendorType === 1 ? chips : -1,
      rulesReady,
      // the tax of the player's area (WP-CFEAT-52)
      `${String(taxOf(ctx)?.area() ?? 0)}:${String(taxOf(ctx)?.purchasePercent() ?? 0)}:${String(taxOf(ctx)?.salesPercent() ?? 0)}`,
      ctx.features.features.quick_sell,
      ctx.features.features.new_font_color,
    ].join('|');
    if (cached?.st === st && cached.key === key) return cached.view;
    const cellOf = (e: ShopStockEntry, tab: number): ShopCell => {
      const hit = cellCache.get(e);
      if (hit?.key === key) return hit.cell;
      const p = prop(e.item.itemId);
      let tint = unusable(e.item, rweapon);
      let price: number;
      if (vendorType === 1) {
        const chip = prices.chip(e.item.itemId);
        price = Math.max(0, chip);
        if (!tint && chips < chip) tint = true;
      } else {
        const b = prices.buy(e.item.itemId);
        price = b.unit + b.tax;
      }
      const cell: ShopCell = {
        name: p?.name ?? `Item ${String(e.item.itemId)}`,
        kind2: p?.item_kind2 ?? 0,
        parts: partsOf(p),
        serial: `shop:${st.npcKey}:${String(tab)}:${String(e.objId)}`,
        objId: e.objId,
        itemId: e.item.itemId,
        icon: itemIconPath(p, e.item),
        count: e.item.num,
        tint,
        sexMatch: p && p.item_sex !== NULL_ID ? p.item_sex === s.sex : null,
        levelOk: p && p.limit_level1 !== NULL_ID ? s.level >= p.limit_level1 : null,
        price,
        item: e.item,
      };
      cellCache.set(e, { key, cell });
      return cell;
    };
    const tabs: ShopTabView[] = [0, 1, 2, 3].map((i) => ({
      title: row?.tabs[i]?.title ?? '',
      index: i,
      cells: (st.tabs[i] ?? []).map((e) => cellOf(e, i)),
    }));
    const view: ShopView = {
      version: st.version,
      npcId: st.npcId,
      vendorType,
      tabs,
      quickSell: ctx.features.features.quick_sell,
      newFontColor: ctx.features.features.new_font_color,
    };
    cached = { st, key, view };
    return view;
  };
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? (() => undefined),
  };
  const taxSource = {
    subscribe: (fn: () => void) => taxOf(ctx)?.subscribe(fn) ?? (() => undefined),
  };
  const sources = [
    store,
    selfStore,
    dataSource,
    taxSource,
    ...(inv() ? [inv()?.store] : []),
  ].filter((x): x is NonNullable<typeof x> => x !== undefined);
  const view = derivedView(build, sources, (a, b) => a === b);

  // ---- closing ------------------------------------------------------------------------------------------------------------------

  const closeDependents = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    for (const app of [APP_CONFIRM_SELL, APP_WARNING, APP_QUICK_SELL, APP_CONFIRM_BUYEX]) {
      ui.wm.get(app)?.destroy();
    }
  };
  const close = (): void => {
    const ui = fullUi(ctx);
    const w = ui?.wm.get(APP_SHOPEX);
    if (w) w.destroy();
    else if (store.getState().open) {
      senders.closeShop();
      store.setState(closedShopState(store.getState()));
    }
  };

  // ---- buying -------------------------------------------------------------------------------------------------------------------

  const buyRequest = (pick: ShopPick, vendorType: 0 | 1): ConfirmBuyRequest => {
    const p = prop(pick.cell.itemId);
    const max = maxBuyQuantity(p, pick.cell.itemId);
    const unit = (): number => {
      if (vendorType === 1) return Math.max(0, prices.chip(pick.cell.itemId));
      const b = prices.buy(pick.cell.itemId);
      return b.unit + b.tax;
    };
    return {
      name: pick.cell.name,
      icon: pick.cell.icon,
      stock: pick.cell.count,
      maxQuantity: max,
      buyType: vendorType,
      total: (n) => unit() * n,
      confirm: (n) => {
        const cost = unit();
        const msg = (tid: number): void => {
          fullUi(ctx)?.msgbox.open(text(tid));
        };
        if (pick.cell.count < 1 || cost === 0) {
          msg(TID_DIAG_0006);
          return;
        }
        if (vendorType === 1) {
          if (n * cost > (inv()?.countOf(II_CHP_RED) ?? 0)) {
            msg(TID_GAME_CANNTBUY_REDCHIP);
            return;
          }
        } else if (n * cost > selfStore.getState().gold) {
          msg(TID_DIAG_0009);
          return;
        }
        if (n < 1 || n > max) {
          msg(TID_DIAG_0086);
          return;
        }
        if (n > pick.cell.count) {
          msg(TID_DIAG_0087);
          return;
        }
        stats.bought += 1;
        // `PLAYSND( SND_INF_TRADE )` after the send of the buy confirm (T24)
        ctx.audio.cues?.shopConfirm();
        if (vendorType === 1) senders.buyChip(pick.tab, pick.cell.objId, n, pick.cell.itemId);
        else senders.buy(pick.tab, pick.cell.objId, n, pick.cell.itemId);
      },
    };
  };

  /** A stock item picked for a purchase (drag to the bag or double click): the affordability gate, then the quantity box. */
  const startBuy = (pick: ShopPick): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    const vendorType = view.get().vendorType;
    if (vendorType === 1) {
      const chip = prices.chip(pick.cell.itemId);
      if ((inv()?.countOf(II_CHP_RED) ?? 0) - chip < 0) {
        ui.msgbox.open(text(TID_GAME_CANNT_BY_REDCHIP));
        return false;
      }
    } else {
      const b = prices.buy(pick.cell.itemId);
      if (selfStore.getState().gold - (b.unit + b.tax) < 0) {
        ui.msgbox.open(text(TID_DIAG_0048));
        return false;
      }
    }
    ui.npc.openConfirmBuy(buyRequest(pick, vendorType));
    return true;
  };

  // ---- selling ---------------------------------------------------------------------------------------------------------------------

  const sellRequest = (objId: number): ConfirmSellRequest | null => {
    const it = inv()?.item(objId);
    if (!it) return null;
    const p = prop(it.itemId);
    const live = (): InvItem | null => inv()?.item(objId) ?? null;
    return {
      name: p?.name ?? `Item ${String(it.itemId)}`,
      icon: itemIconPath(p, it),
      count: () => live()?.num ?? 0,
      reserved: () => (live()?.extra ?? 0) > 0,
      gone: () => live() === null,
      total: (n) => {
        const cur = live();
        if (!cur) return 0;
        // `max(unit, 1)` per piece, then the quantity
        return Math.max(1, prices.sell(cur)) * n;
      },
      confirm: (n) => {
        stats.sold += 1;
        // `PLAYSND( SND_INF_TRADE )` at the OK of the sell confirm (T24)
        ctx.audio.cues?.shopConfirm();
        senders.sell(objId, n);
      },
    };
  };

  /** `WIN_ITEMDROP` on the shop: the gates of `CWndShop::OnChildNotify`. Returns whether the drop was accepted. */
  const sell = (objId: number): boolean => {
    const ui = fullUi(ctx);
    const it = inv()?.item(objId);
    if (!ui || !it) return false;
    const p = prop(it.itemId);
    if (isQuestItem(p)) return false;
    if (inv()?.state().container.isEquip(objId) === true) {
      say(TID_GAME_EQUIPTRADE);
      return false;
    }
    // a premium item, and a unique of level 120 or more (`__QUICK_SELL`), asks first
    const warn =
      isCharged(it) ||
      (p !== undefined &&
        p.refer_stat1 !== NULL_ID &&
        p.refer_stat1 >= WEAPON_UNIQUE &&
        p.limit_level1 >= 120);
    for (const app of [APP_QUICK_SELL, APP_WARNING, APP_CONFIRM_SELL]) ui.wm.get(app)?.destroy();
    ui.wm.flushDestroyed();
    const open = (): void => {
      const req = sellRequest(objId);
      if (req) ui.npc.openConfirmSell(req);
    };
    if (warn) ui.npc.openWarning({ ok: open });
    else open();
    return true;
  };

  // ---- the quick sale ----------------------------------------------------------------------------------------------------------

  let reserved: InvItem[] = [];
  let quickState: QuickSellView = { version: 0, entries: [], total: 0 };
  let quickKey = '';
  const quickFns = new Set<() => void>();
  let quickOff: (() => void) | null = null;
  /** The entries and the total from the reserved items (their current records); notifies when something changed. */
  const refreshQuick = (): void => {
    const live = reserved.map((it) => inv()?.item(it.objId) ?? it);
    const entries = live.map((it) => {
      const p = prop(it.itemId);
      return {
        objId: it.objId,
        icon: itemIconPath(p, it),
        count: it.num,
        showCount: (p?.pack_max ?? 1) > 1,
        item: it,
        name: p?.name ?? `Item ${String(it.itemId)}`,
        kind2: p?.item_kind2 ?? 0,
        parts: partsOf(p),
        serial: it.serial,
      };
    });
    const total = quickSellTotal(live, (it) => prices.quick(it));
    const key = `${entries.map((e) => `${String(e.objId)}:${String(e.count)}`).join(',')}|${String(total)}`;
    if (key === quickKey) return;
    quickKey = key;
    quickState = { version: quickState.version + 1, entries, total };
    for (const fn of [...quickFns]) fn();
  };
  const quickView: ViewSource<QuickSellView> = {
    get: () => quickState,
    subscribe(fn) {
      quickFns.add(fn);
      return () => {
        quickFns.delete(fn);
      };
    },
  };
  const restoreReservations = (): void => {
    quickOff?.();
    quickOff = null;
    for (const it of reserved) inv()?.setExtra(it.objId, 0);
    reserved = [];
    quickKey = '';
    quickState = { version: quickState.version + 1, entries: [], total: 0 };
  };

  const quickActions: QuickSellActions = {
    confirm() {
      stats.quickSold += 1;
      senders.quickSell();
    },
    closed() {
      restoreReservations();
    },
  };
  const openQuickSell = (): boolean => {
    const ui = fullUi(ctx);
    const i = inv();
    if (!ui || !i || !store.getState().open) return false;
    for (const app of [APP_WARNING, APP_CONFIRM_SELL, APP_QUICK_SELL]) ui.wm.get(app)?.destroy();
    ui.wm.flushDestroyed();
    restoreReservations();
    reserved = quickSellCandidates(i.state().container, prop);
    // `pItemElem->SetExtra(pItemElem->m_nItemNum)`: the whole stack is reserved while the window is open
    for (const it of reserved) i.setExtra(it.objId, it.num);
    refreshQuick();
    quickOff = i.store.subscribe(refreshQuick);
    return ui.npc.openQuickSell({ view: quickView, actions: quickActions }) !== null;
  };

  // ---- the actions of the shop window ----------------------------------------------------------------------------------

  const actions: ShopActions = {
    buy: (pick) => {
      startBuy(pick);
    },
    sell,
    quickSell: () => {
      openQuickSell();
    },
    closed() {
      // `CWndShop::OnDestroy`: the partner link ends (`CloseShopWnd`), the confirmation windows go with the shop
      if (store.getState().open) senders.closeShop();
      store.setState(closedShopState(store.getState()));
      closeDependents();
      restoreReservations();
    },
  };

  // ---- opening ----------------------------------------------------------------------------------------------------------------

  /** `MMI_TRADE` on an NPC (`CWndWorld::OnCommand`): the busy checks, the chaotic penalty, then the request. */
  const requestOpen = (npcObjId: number): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    const busy = busyText((app) => ui.wm.isOpen(app));
    if (busy !== null) {
      say(busy);
      return false;
    }
    // `m_vtInfo.GetOther() == NULL`: an open shop is the partner
    if (ui.wm.isOpen(APP_SHOPEX) || store.getState().open) return false;
    const pk = selfStore.getState().pk;
    if (pk.propensity > 0) {
      const rows = ctx.data?.bundle('tables')?.pk.penalties ?? [];
      const row =
        rows.find((r) => r.min <= pk.propensity && pk.propensity <= r.max) ??
        rows.find((r) => r.step === 8);
      // without the penalty table the server decides (it checks the same row)
      if (rows.length > 0 && (!row || row.shop === 0)) {
        say(TID_GMAE_CHAOTIC_NOT_SHOP);
        return false;
      }
    }
    senders.openShop(npcObjId);
    return true;
  };

  menu.registerAction(MMI_TRADE, ({ targetId, npcKey }) => {
    if (npcKey !== null) requestOpen(targetId);
  });

  fc.net.on('openShopWndNtf', (m, objId) => {
    const rec = ctx.entities.get(objId);
    if (rec?.kind !== EntityKind.Npc) return;
    const key = (rec.spawn as SpawnNpc).npc?.characterKey ?? '';
    const ui = fullUi(ctx);
    // a shop that is still open (or waits for its deferred destruction) is replaced
    const old = ui?.wm.get(APP_SHOPEX);
    if (old) {
      old.destroy();
      ui?.wm.flushDestroyed();
    }
    store.setState(applyOpenShop(store.getState(), objId, key, m.tabs));
    stats.opened += 1;
    if (!ui) return;
    // the inventory opens beside the shop (45 §12.1), the shop takes its place next to it
    ui.char.openInventory();
    ui.npc.openShop({ view, actions });
  });
  fc.net.on('vendor', (m, objId) => {
    store.setState(applyVendor(store.getState(), objId, m.tabs));
  });
  fc.net.on('updateVendor', (m, objId) => {
    store.setState(applyUpdateVendor(store.getState(), objId, m.tab, m.objId, m.num));
  });

  // the NPC leaves the area: the shop goes with it
  ctx.entities.onRemove((rec) => {
    const st = store.getState();
    if (st.open && rec.objId === st.npcId) close();
  });
  fc.transactional.register({ id: 'npc.shop', reasons: 'all', close });

  // a stock item dropped on the bag grid is a purchase (`CWndInventory::OnChildNotify`, WIN_ITEMDROP, `APP_SHOP_` branch)
  const dropIn = (sc: Shortcut): boolean => {
    if (sc.kind !== SC.ITEM) return false;
    const st = view.get();
    const tab = st.tabs[sc.userId];
    const cell = tab?.cells.find((c) => c.objId === sc.id);
    if (!cell) return false;
    return startBuy({ tab: sc.userId, cell });
  };
  let unregisterDrop: (() => void) | null = null;
  const hookInventory = (): void => {
    const i = inv();
    if (!i || unregisterDrop) return;
    unregisterDrop = i.registerDropIn(APP_SHOPEX, dropIn);
  };
  ctx.tickSystems.register('npc.shop.inventory', hookInventory);
  hookInventory();

  const feature: ShopFeature = {
    store,
    view,
    actions,
    senders,
    prices,
    requestOpen,
    close,
    isOpen: () => store.getState().open,
    state: () => store.getState(),
    quickSellOpen: () => fullUi(ctx)?.wm.isOpen(APP_QUICK_SELL) ?? false,
    openQuickSell,
  };
  features.set(ctx, feature);

  addProbeMember(ctx, 'shop', () => {
    const s = store.getState();
    const v = view.get();
    return {
      open: s.open,
      npcId: s.npcId,
      npcKey: s.npcKey,
      vendorType: v.vendorType,
      tabs: v.tabs.map((t) => ({
        title: t.title,
        cells: t.cells.map((c) => ({
          objId: c.objId,
          itemId: c.itemId,
          name: c.name,
          price: c.price,
          count: c.count,
          tint: c.tint,
        })),
      })),
      quickSell: reserved.map((it) => ({ objId: it.objId, itemId: it.itemId, num: it.num })),
      stats: { ...stats },
    };
  });
  return feature;
}
