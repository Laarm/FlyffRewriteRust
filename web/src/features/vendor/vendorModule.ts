// The `vendor` feature module (Plan/45 §15, 19 §9; ref/09b §3.9; WP-CFEAT-22): the personal shop and the chat room of a shop.
//
//   own shop       the shop window (`APP_VENDOR_REVISION`, from the start menu): items dropped on a free slot ask the price (`APP_VENDOREX_SELL`) and
//                  register (`RegisterPVendorItemReq`), the title and OK open the shop after a confirmation (`PVendorOpenReq`), Cancel of an open
//                  shop asks "stop selling?" and closes it (`PVendorCloseReq`); the goods of the last session are registered again when the window
//                  opens (`ReloadItemList`)
//   RegisterPVendorItemNtf / UnregisterPVendorItemNtf   the own slots (an own good is reserved in the bag)
//   PVendorOpenNtf / PVendorCloseNtf    any shop: the title balloon over the vendor (`RenderFx.vendorTitle`); the own one: open / closed
//   PVendorItem    a visited shop: the shop window in visitor mode, `EnterChatting`, the chat window when the room is open
//   PVendorItemNum a purchase: the slot's count; the owner also gets the sale line in the info page of the chat window
//   ChattingNtf    the room: members, talk lines, the owner's info lines, the open state (a closed room closes the window)
//
// A purchase (`APP_VENDOREX_BUY`) checks the wallet first (`TID_GAME_LACKMONEY` over the window) and sends `BuyPVendorItem`.
// The decisions of the rules stay on the server (D1); the client keeps the legacy gates (the total-gold rule of `total.ts`, the title check of `title.ts`,
// the tradeable function of the rules before an item is offered).
//
// Other features reach it with `vendorOf(ctx)`.

import type { ClientContext } from '../../app/context';
import { entityInfoOf } from '../../app/inputHost';
import { inputProvidersOf } from '../../app/inputProviders';
import { EntityKind } from '../../game/entities';
import { regionAt } from '../../game/world/region';
import {
  APP_COMMON_BANK,
  APP_GUILD_BANK,
  APP_INVENTORY,
  APP_SHOPEX,
  APP_TRADE,
  APP_VENDOR_REVISION,
} from '../../gen/defs/resdata';
import {
  TID_GAME_BUY_INFO,
  TID_GAME_FROM3,
  TID_GAME_LACKMONEY,
  TID_GAME_VENCHATIN,
  TID_GAME_VENCHATOUT,
  TID_GAME_VENDOR_MAX_ALL_GOLD,
  TID_GAME_VENDOR_START,
  TID_GAME_VENDOR_STOP,
  TID_GAME_VENDOR_TITLE,
  TID_GAME_VENDOR_TITLE0,
} from '../../gen/defs/text';
import { MB, MBID } from '../../ui/msgbox/types';
import type {
  VendorActions,
  VendorBuyRequest,
  VendorChatPorts,
  VendorChatView,
  VendorEntry,
  VendorPorts,
  VendorSellRequest,
  VendorView,
  ViewSource,
} from '../../ui/windows/vendor/ports';
import { SC, type Shortcut } from '../../ui/dnd/shortcut';
import type { WndNeuz } from '../../ui/wnd/WndNeuz';
import { createWordFilter } from '../chat/wordFilter';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { formatTid, sysChat } from '../common/text';
import { foreignItemFields, tradeRefusal } from '../common/tradeItems';
import { derivedView } from '../common/view';
import { inventoryOf } from '../inventory';
import { isExpired, isGuildCloak, isQuestItem } from '../inventory/info';
import { itemFromWire } from '../inventory/items';
import { duelOf } from '../pvp/duel/duelModule';
import { tradeOf } from '../trade';
import {
  clearSaved,
  endBrowse,
  endChat,
  forgetSaved,
  onBrowse,
  onCloseSelf,
  onDeleteChatting,
  onEnterMember,
  onInfo,
  onItemNum,
  onNewChatting,
  onOpenSelf,
  onRegister,
  onRemoveMember,
  onRoomState,
  onTalk,
  onUnregister,
  resetAll,
  saveGoods,
  setStopping,
} from './reducers';
import { createVendorSenders, type VendorSenders } from './senders';
import {
  defineVendorStore,
  MAX_VENDITEM,
  MAX_VENDOR_REVISION,
  type BrowseGood,
  type OwnGood,
  type VendorStore,
} from './store';
import { checkVendorTitle, type TitleRules } from './title';
import { openingAllowed, parsePrice, registrationAllowed, type Listed } from './total';

/** The public face of the vendor feature of a context. */
export interface VendorFeature {
  readonly store: VendorStore;
  readonly view: ViewSource<VendorView>;
  readonly actions: VendorActions;
  readonly chatPorts: VendorChatPorts;
  readonly chatView: ViewSource<VendorChatView>;
  readonly senders: VendorSenders;
  /** Opens the shop window of the own shop (`CreateApplet( APP_VENDOR_REVISION )`); `false` when it refuses (a duel, a trade ...). */
  openOwn(): boolean;
  /** The own shop is open (`IsVendorOpen`). */
  isOpen(): boolean;
  /** A shop is being visited. */
  isBrowsing(): boolean;
  /** `QueryPVendorItem`: look into the shop of a player. */
  visit(vendorObjId: number): void;
}

const features = new WeakMap<ClientContext, VendorFeature>();

/** The vendor feature of a context; `undefined` before the module registered. */
export function vendorOf(ctx: ClientContext): VendorFeature | undefined {
  return features.get(ctx);
}

export const vendorModule: FeatureModule = {
  id: 'vendor',
  milestone: 4,
  register(fc) {
    registerVendor(fc);
  },
};

/** Red for the own lines and the joins, blue for the others' lines and the sales (`#cffff0000` / `#cff0000ff`). */
const RED = '#cffff0000';
const BLUE = '#cff0000ff';

/** The windows that stop the shop window from opening (`g_WndMng.m_pWndTrade || m_pWndShop || m_pWndBank || m_pWndGuildBank`). */
const BLOCKING: readonly number[] = [APP_TRADE, APP_SHOPEX, APP_COMMON_BANK, APP_GUILD_BANK];

function registerVendor(fc: FeatureContext): VendorFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const store = defineVendorStore(ctx.stores.registry);
  const senders = createVendorSenders(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number, args: readonly string[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };
  const box = (tid: number, parent?: WndNeuz): void => {
    fullUi(ctx)?.msgbox.open(text(tid), MB.OK, parent);
  };
  const inv = () => inventoryOf(ctx);
  const stats = { opened: 0, registered: 0, sales: 0, purchases: 0, visits: 0 };

  const selfObj = (): number => selfStore.getState().objId;

  // ---- the read models ---------------------------------------------------------------------------------------------------------

  const names = (): {
    rules: TitleRules | null;
    filter: ReturnType<typeof createWordFilter> | null;
  } => {
    const n = ctx.data?.bundle('names');
    return {
      rules: n
        ? {
            invalidSubstrings: n.invalid_substrings,
            allowedLetters: n.vendor_allowed_letters ?? null,
          }
        : null,
      filter: n?.chat_filter ? createWordFilter(n.chat_filter) : null,
    };
  };

  const entryOf = (slot: number, g: OwnGood): VendorEntry | null => {
    const it = inv()?.item(g.objId);
    if (!it) return null;
    const f = foreignItemFields(ctx.data, it, `vendor:own:${String(slot)}`);
    return { ...f, slot, count: g.count, price: g.price, option: it.abilityOption };
  };
  const browseEntry = (vendor: number, slot: number, g: BrowseGood): VendorEntry => {
    const f = foreignItemFields(ctx.data, g.item, `vendor:${String(vendor)}:${String(slot)}`);
    return { ...f, slot, count: g.count, price: g.price, option: g.item.abilityOption };
  };

  const build = (): VendorView => {
    const s = store.getState();
    const cells: (VendorEntry | null)[] = [];
    if (s.browse) {
      for (let i = 0; i < MAX_VENDOR_REVISION; i += 1) {
        const g = s.browse.goods[i];
        cells.push(g ? browseEntry(s.browse.vendor, i, g) : null);
      }
      return {
        version: s.version,
        owner: false,
        title: s.browse.title,
        open: false,
        windowTitle: text(TID_GAME_VENDOR_TITLE) + text(TID_GAME_VENDOR_TITLE0),
        cells,
      };
    }
    for (let i = 0; i < MAX_VENDOR_REVISION; i += 1) {
      const g = s.own[i];
      cells.push(g ? entryOf(i, g) : null);
    }
    return {
      version: s.version,
      owner: true,
      title: s.title,
      open: s.open,
      windowTitle: s.open
        ? text(TID_GAME_VENDOR_TITLE) + text(TID_GAME_VENDOR_TITLE0)
        : text(TID_GAME_VENDOR_TITLE),
      cells,
    };
  };
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? (() => undefined),
  };
  const invSource = {
    subscribe: (fn: () => void) => inv()?.store.subscribe(fn) ?? (() => undefined),
  };
  const view = derivedView(build, [store, dataSource, invSource], (a, b) => a === b);

  const chatBuild = (): VendorChatView => {
    const s = store.getState();
    return {
      version: s.version,
      owner: s.browse === null,
      members: s.chat.members.map((m) => m.name),
      talk: s.chat.talk,
      info: s.chat.info,
    };
  };
  let lastChat: {
    talk: readonly string[];
    info: readonly string[];
    members: unknown;
    v: VendorChatView;
  } | null = null;
  const chatView = derivedView(
    () => {
      const next = chatBuild();
      const c = store.getState().chat;
      const prev = lastChat;
      if (
        prev?.talk === c.talk &&
        prev.info === c.info &&
        prev.members === c.members &&
        prev.v.owner === next.owner
      ) {
        return prev.v;
      }
      lastChat = { talk: c.talk, info: c.info, members: c.members, v: next };
      return next;
    },
    [store],
    (a, b) => a === b,
  );

  // ---- reservations of the own goods ------------------------------------------------------------------------------------------

  // an own good is reserved in the bag (`SetExtra( count )`), a vanished one lifts it
  store.subscribe((next, prev) => {
    for (let i = 0; i < MAX_VENDITEM; i += 1) {
      const a = prev.own[i];
      const b = next.own[i];
      if (a === b) continue;
      if (a && b?.objId !== a.objId) inv()?.setExtra(a.objId, 0);
      if (b) inv()?.setExtra(b.objId, b.count);
    }
  });

  // ---- the windows ------------------------------------------------------------------------------------------------------------------

  const listed = (): (Listed | null)[] =>
    store.getState().own.map((g) => (g ? { price: g.price, count: g.count } : null));
  const hasGoods = (): boolean => store.getState().own.some((g) => g !== null);

  const closeDialogs = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    ui.vendor.closeSell();
    ui.vendor.closeBuy();
    ui.vendor.closeChat();
  };

  /** The chat window of the shop (the owner's second page, the room's members and lines). */
  const chatPorts: VendorChatPorts = {
    view: chatView,
    actions: {
      send(line) {
        senders.chatting(line);
      },
      closedByUser() {
        // `WTBID_CLOSE` of the chat window: the owner closes the room
        if (store.getState().open && store.getState().browse === null) senders.closeChattingRoom();
      },
      closed() {
        // nothing: the room stays (the Chat button shows the window again)
      },
    },
  };
  const openChat = (): void => {
    fullUi(ctx)?.vendor.openChat(chatPorts);
  };

  // ---- the actions of the shop window -------------------------------------------------------------------------------------------

  const duelling = (): boolean => (duelOf(ctx)?.store.getState().duel ?? 0) === 2;

  const registerDrop = (sc: Shortcut, slot: number): boolean => {
    const i = inv();
    const ui = fullUi(ctx);
    const s = store.getState();
    if (!i || !ui || s.browse || s.open) return false;
    if (sc.kind !== SC.ITEM || sc.data === 0) return false;
    if (slot < 0 || slot >= MAX_VENDOR_REVISION || s.own[slot]) return false;
    const it = i.item(sc.id);
    if (!it) return false;
    const prop = ctx.data?.item(it.itemId);
    // the checks of `CWndVendor::OnChildNotify`: a quest item, a guild cloak (silently), an expired item (taken, nothing happens)
    if (isQuestItem(prop) || isGuildCloak(it, prop)) return false;
    if (isExpired(it)) return true;
    // an item in use (a reserved stack, a summoned pet ...) is not offered
    if (it.extra !== 0) return false;
    const refusal = tradeRefusal(ctx, it, { equipped: i.state().container.isEquip(it.objId) });
    if (refusal !== 0) {
      say(refusal);
      return false;
    }
    const sell: VendorSellRequest = {
      slot,
      name: prop?.name ?? '',
      stack: it.num,
      register: (at, quantity, priceText) => registerGoods(it.objId, at, quantity, priceText),
    };
    ui.vendor.openSell(sell);
    return true;
  };

  /** `WIDC_OK` of the price window: the total-gold rule, then `RegisterPVendorItemReq`. */
  const registerGoods = (
    objId: number,
    slot: number,
    quantity: number,
    priceText: string,
  ): boolean => {
    const price = parsePrice(priceText);
    if (price < 1 || quantity < 1) return false;
    const wallet = selfStore.getState().gold;
    if (!registrationAllowed(wallet, listed(), price, quantity)) {
      box(TID_GAME_VENDOR_MAX_ALL_GOLD, fullUi(ctx)?.vendor.sell() ?? undefined);
      return false;
    }
    senders.register(slot, objId, quantity, price);
    stats.registered += 1;
    return true;
  };

  /** `ReloadItemList`: the goods of the last session are registered again while the shop is closed. */
  const reload = (): void => {
    const s = store.getState();
    if (s.open || s.browse) return;
    const i = inv();
    if (!i) return;
    for (let slot = 0; slot < MAX_VENDITEM; slot += 1) {
      const sv = s.saved[slot];
      if (!sv) continue;
      const it = i.item(sv.objId);
      if (it?.itemId !== sv.itemId) continue;
      if (sv.count > it.num) {
        store.setState(forgetSaved(store.getState(), slot));
        continue;
      }
      senders.register(slot, sv.objId, sv.count, sv.price);
    }
  };

  const start = (rawTitle: string): boolean => {
    const s = store.getState();
    // flying, or nothing registered: the button does nothing
    if (selfStore.getState().isFlying || !hasGoods() || s.browse || s.open) return false;
    const owner = fullUi(ctx)?.vendor.vendor() ?? undefined;
    if (!openingAllowed(selfStore.getState().gold, listed())) {
      box(TID_GAME_VENDOR_MAX_ALL_GOLD, owner);
      return false;
    }
    const n = names();
    const check = checkVendorTitle(rawTitle, n.rules, n.filter);
    if (!check.ok) {
      box(check.tid, owner);
      return false;
    }
    fullUi(ctx)?.msgbox.open(text(TID_GAME_VENDOR_START), MB.OKCANCEL, owner, (id) => {
      if (id === MBID.OK) {
        // the window may have closed behind the box
        if (store.getState().browse === null && !store.getState().open) senders.open(check.title);
      } else {
        fullUi(ctx)?.vendor.vendor()?.restoreOk();
      }
    });
    return true;
  };

  const askStop = (): void => {
    const ui = fullUi(ctx);
    if (!ui || !store.getState().open) return;
    ui.msgbox.open(
      text(TID_GAME_VENDOR_STOP),
      MB.OKCANCEL,
      ui.vendor.vendor() ?? undefined,
      (id) => {
        if (id !== MBID.OK) return;
        if (!store.getState().open) return;
        store.setState(setStopping(store.getState(), true));
        ui.vendor.vendor()?.destroy();
      },
    );
  };

  const buyWindow = (slot: number): boolean => {
    const ui = fullUi(ctx);
    const b = store.getState().browse;
    const g = b?.goods[slot];
    if (!ui || !b || !g) return false;
    const prop = ctx.data?.item(g.item.itemId);
    const req: VendorBuyRequest = {
      slot,
      name: prop?.name ?? '',
      count: g.count,
      price: g.price,
      confirm: (at, quantity) => {
        const cur = store.getState().browse;
        const good = cur?.goods[at];
        if (!cur || !good) return true; // the goods are gone (sold out meanwhile): the window just closes
        const n = Math.min(quantity, good.count);
        // `GetGold() < m_nCost * nBufNum` (exact here: the original multiplied two `int`)
        if (selfStore.getState().gold < good.price * n) {
          box(TID_GAME_LACKMONEY, ui.vendor.buy() ?? undefined);
          return false;
        }
        senders.buy(cur.vendor, at, good.item.itemId, quantity);
        stats.purchases += 1;
        return true;
      },
    };
    ui.vendor.openBuy(req);
    return true;
  };

  const actions: VendorActions = {
    opened: reload,
    registerDrop,
    unregister(slot) {
      const s = store.getState();
      if (!s.browse && !s.open && s.own[slot]) senders.unregister(slot);
    },
    buy: buyWindow,
    reset() {
      const s = store.getState();
      if (s.browse || s.open) return;
      s.own.forEach((g, slot) => {
        if (g) senders.unregister(slot);
      });
      store.setState(clearSaved(store.getState()));
    },
    start,
    cancel: askStop,
    toggleChat() {
      const s = store.getState();
      let room = false;
      if (s.browse === null && s.open) {
        // the owner: the room opens for talk
        senders.openChattingRoom();
        room = true;
      } else if (s.browse !== null && s.chat.open) {
        room = true;
      }
      if (!room) return;
      fullUi(ctx)?.vendor.toggleChat(chatPorts);
    },
    closed() {
      const s = store.getState();
      closeDialogs();
      if (s.browse) {
        // leave the visited shop (`SendPVendorClose( vendor )`), the room with it
        senders.close(s.browse.vendor);
        store.setState(endBrowse(store.getState()));
        return;
      }
      // `OnDestroy`: the goods are saved for the next opening; the shop closes with the window
      store.setState(saveGoods(s, (id) => inv()?.item(id)?.itemId ?? 0));
      if (s.open || hasGoods()) senders.close(selfObj());
    },
  };

  const ports: VendorPorts = {
    view,
    actions,
    canOpen: () => {
      const ui = fullUi(ctx);
      if (!ui) return false;
      // `CWndVendor::Initialize`: not during a duel, not with a trade, shop, bank or guild bank window open
      return !duelling() && !ui.wm.anyOpen(BLOCKING);
    },
  };

  const openOwn = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    ui.wm.openApplet(APP_INVENTORY);
    ui.vendor.setPorts(ports);
    return ui.vendor.openVendor() !== null;
  };

  // ---- the drop of a good on the bag (`CWndInventory::OnDropIcon` for a shortcut of the shop window) -----------------------------

  inv()?.registerDropIn(APP_VENDOR_REVISION, (sc) => {
    const s = store.getState();
    if (s.browse) return buyWindow(sc.index);
    if (!s.open) actions.unregister(sc.index);
    return true;
  });

  // ---- the messages --------------------------------------------------------------------------------------------------------------

  fc.net.on('registerPVendorItemNtf', (m) => {
    store.setState(
      onRegister(store.getState(), m.slot, { objId: m.objId, num: m.num, cost: m.cost }),
    );
  });

  fc.net.on('unregisterPVendorItemNtf', (m) => {
    store.setState(onUnregister(store.getState(), m.slot));
  });

  fc.net.on('pVendorOpenNtf', (m, objId) => {
    const rec = ctx.entities.get(objId);
    if (rec?.visual) rec.visual.vendorTitle = m.title;
    if (objId !== selfObj()) return;
    store.setState(onOpenSelf(store.getState(), m.title));
    stats.opened += 1;
    const ui = fullUi(ctx);
    if (!ui) return;
    // the owner joins the room of the own shop and sees its window (`SendEnterChattingRoom( self )`; the room exists already)
    senders.enterChatting(selfStore.getState().idPlayer);
    store.setState(onRoomState(store.getState(), true));
    openChat();
  });

  fc.net.on('pVendorCloseNtf', (m, objId) => {
    const ui = fullUi(ctx);
    const s = store.getState();
    const mine = objId === selfObj();
    if (!m.closed) {
      // the window of a viewer must close (the vendor's shop closed, the vendor is out of view, we left)
      if (s.browse?.vendor === objId) {
        ui?.vendor.vendor()?.destroy();
        store.setState(endBrowse(store.getState()));
      }
      return;
    }
    // the shop is closed: the title balloon goes
    const rec = ctx.entities.get(objId);
    if (rec?.visual) rec.visual.vendorTitle = null;
    if (s.browse?.vendor === objId) {
      ui?.vendor.vendor()?.destroy();
      store.setState(endBrowse(store.getState()));
    }
    if (!mine) return;
    const wasOpen = s.open;
    const stopping = s.stopping;
    store.setState(endChat(onCloseSelf(store.getState())));
    ui?.vendor.closeChat();
    // "stop selling": the window comes back in set-up mode with the title and the goods of the shop
    if (wasOpen && stopping && ui?.vendor.vendor() === null) openOwn();
  });

  fc.net.on('pVendorItem', (m, objId) => {
    const rec = ctx.entities.get(objId);
    if (rec?.kind !== EntityKind.Player) return;
    const goods: (BrowseGood | null)[] = Array.from({ length: MAX_VENDITEM }, () => null);
    for (const e of m.entries) {
      if (e.slot < 0 || e.slot >= MAX_VENDITEM || !e.item) continue;
      goods[e.slot] = { item: itemFromWire(e.item), count: e.count, price: e.price };
    }
    const ui = fullUi(ctx);
    // a window of another visit goes first
    if (store.getState().browse) ui?.vendor.vendor()?.destroy();
    const vendorId = rec.idPlayer ?? 0;
    store.setState(
      onBrowse(store.getState(), {
        vendor: objId,
        vendorId,
        title: rec.visual?.vendorTitle ?? '',
        goods,
        chatOpen: m.chattingState,
      }),
    );
    stats.visits += 1;
    if (!ui) return;
    ui.wm.openApplet(APP_INVENTORY);
    ui.vendor.setPorts(ports);
    if (ui.vendor.openVendor() === null) {
      // the window refused (a duel, a trade, a shop): the visit ends on the server too
      senders.close(objId);
      store.setState(endBrowse(store.getState()));
      return;
    }
    senders.enterChatting(vendorId);
    if (m.chattingState) openChat();
  });

  fc.net.on('pVendorItemNum', (m, objId) => {
    const s = store.getState();
    if (objId === selfObj()) {
      const g = s.own[m.slot];
      const it = g ? inv()?.item(g.objId) : null;
      if (g && it) {
        // the sale line of the owner: `WriteBuyInfo( buyer, item, n, n * price )`
        const sold = g.count - m.remaining;
        const prop = ctx.data?.item(it.itemId);
        const line = formatTid(fc.texts, TID_GAME_BUY_INFO, [
          m.buyerName,
          prop?.name ?? '',
          sold,
          sold * g.price,
        ]);
        store.setState(onInfo(store.getState(), `${BLUE}${line}\n`));
        stats.sales += 1;
      }
    }
    store.setState(onItemNum(store.getState(), objId === selfObj(), m.slot, m.remaining));
  });

  fc.net.on('chattingNtf', (m) => {
    const sub = m.sub;
    const s = store.getState();
    const ownerWindow = s.browse === null;
    switch (sub.case) {
      case 'newChatting':
        store.setState(
          onNewChatting(
            s,
            sub.value.members.map((x) => ({ id: x.idPlayer, name: x.name })),
          ),
        );
        break;
      case 'enterMember': {
        const line = ownerWindow
          ? `${RED}${formatTid(fc.texts, TID_GAME_VENCHATIN, [sub.value.name])}\n`
          : null;
        store.setState(onEnterMember(s, { id: sub.value.idPlayer, name: sub.value.name }, line));
        break;
      }
      case 'deleteChatting':
        store.setState(onDeleteChatting(s));
        break;
      case 'removeMember': {
        const who = s.chat.members.find((x) => x.id === sub.value.idPlayer);
        const line =
          ownerWindow && who
            ? `${RED}${formatTid(fc.texts, TID_GAME_VENCHATOUT, [who.name])}\n`
            : null;
        store.setState(onRemoveMember(s, sub.value.idPlayer, line));
        break;
      }
      case 'chatting': {
        // only a known member's line shows (`g_Chatting.m_idMember[i] == uidPlayer`)
        const who = s.chat.members.find((x) => x.id === sub.value.idPlayer);
        if (!who) break;
        const mine = who.id === selfStore.getState().idPlayer;
        store.setState(
          onTalk(
            s,
            `${mine ? RED : BLUE}${who.name}${text(TID_GAME_FROM3)} :#nc\n  ${sub.value.text}\n`,
          ),
        );
        break;
      }
      case 'chattingRoomState': {
        store.setState(onRoomState(s, sub.value.open));
        // a closed room closes the window (`SAFE_DELETE( m_pwndVenderMessage )`)
        if (!sub.value.open) fullUi(ctx)?.vendor.closeChat();
        break;
      }
      case undefined:
        break;
    }
  });

  fc.net.on('delObj', (_m, objId) => {
    const b = store.getState().browse;
    if (b?.vendor === objId) {
      // the visited vendor left the view: the window goes (the server ends the visit)
      fullUi(ctx)?.vendor.vendor()?.destroy();
      store.setState(endBrowse(store.getState()));
    }
  });

  // ---- the title balloons ------------------------------------------------------------------------------------------------------

  // `RenderFx.vendorTitle` follows the entities' titles (a spawn, an open, a close); the renderer may come up after the first spawns, so the table
  // is compared every tick and a title that could not be handed over is retried
  const shown = new Map<number, string>();
  ctx.tickSystems.register('vendor.titles', () => {
    const fx = ctx.render?.fx;
    if (!fx) return;
    const seen = new Set<number>();
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      const title = rec.visual?.vendorTitle ?? null;
      seen.add(rec.objId);
      const had = shown.get(rec.objId) ?? null;
      if (title === had) return;
      fx.vendorTitle(rec.objId, title);
      if (title === null) shown.delete(rec.objId);
      else shown.set(rec.objId, title);
    });
    for (const id of [...shown.keys()]) if (!seen.has(id)) shown.delete(id);
  });

  // ---- the windows -------------------------------------------------------------------------------------------------------------

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.vendor.setPorts(ports);
  };
  ctx.tickSystems.register('vendor.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  // a transactional close (the own death, a world change, the logout, a duel): the visited shop is left, the own window closes
  fc.transactional.register({
    id: 'vendor',
    reasons: 'all',
    close: () => {
      const ui = fullUi(ctx);
      ui?.vendor.vendor()?.destroy();
      closeDialogs();
      store.setState(resetAll(store.getState()));
    },
  });

  const feature: VendorFeature = {
    store,
    view,
    actions,
    chatPorts,
    chatView,
    senders,
    openOwn,
    isOpen: () => store.getState().open,
    isBrowsing: () => store.getState().browse !== null,
    visit: (vendorObjId) => {
      senders.query(vendorObjId);
    },
  };
  features.set(ctx, feature);

  // the input layer (43 §4.4, §7.13, WP-INPUT-12): the own shop stops the world controls, a visited shop or a trade partner (`GetOther()`)
  // turns the shop click of another player off, and a shop in a safe region ignores Ctrl
  const providers = inputProvidersOf(ctx);
  providers.vendor = {
    ownOpen: () => store.getState().open,
    otherOpen: () => store.getState().browse !== null || (tradeOf(ctx)?.isActive() ?? false),
  };
  const infoOf = entityInfoOf(ctx);
  providers.regionSafe = (id) => {
    const w = ctx.stores.worldStore.getState();
    const folder = ctx.data?.worldFolder(w.folder);
    const pos = infoOf(id)?.pos;
    if (!folder || !pos) return false;
    return regionAt(folder.regions, folder.settings.pk_mode, pos[0], pos[2]).safe;
  };

  addProbeMember(ctx, 'vendor', () => {
    const s = store.getState();
    return {
      open: s.open,
      title: s.title,
      own: s.own.map((g, i) => (g ? { slot: i, ...g } : null)).filter((g) => g !== null),
      saved: s.saved.filter((g) => g !== null).length,
      browse: s.browse
        ? {
            vendor: s.browse.vendor,
            title: s.browse.title,
            goods: s.browse.goods
              .map((g, i) =>
                g ? { slot: i, itemId: g.item.itemId, count: g.count, price: g.price } : null,
              )
              .filter((g) => g !== null),
          }
        : null,
      chat: {
        open: s.chat.open,
        members: s.chat.members.map((m) => m.name),
        talk: s.chat.talk.length,
        info: s.chat.info.length,
        talkLines: s.chat.talk.slice(-20),
        infoLines: s.chat.info.slice(-20),
      },
      windows: {
        vendor: fullUi(ctx)?.vendor.vendor() !== null,
        sell: fullUi(ctx)?.vendor.sell() !== null,
        buy: fullUi(ctx)?.vendor.buy() !== null,
        chat: fullUi(ctx)?.vendor.chat() !== null,
      },
      stats: { ...stats },
    };
  });
  return feature;
}
