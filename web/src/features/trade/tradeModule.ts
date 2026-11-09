// The `trade` feature module (Plan/45 §14, 19 §8; ref/09a §5.4; WP-CFEAT-21): player-to-player trade.
//
//   MMI_TRADE (player menu)   the busy checks of the original, then `ConfirmTradeReq` and the line `TID_GAME_TRADEREQUEST`
//   ConfirmTradeNtf           the auto-decline rule (a trade or a request window open: `BlockReq{ 5 }`, the guild bank or the angel window open:
//                             `BlockReq{ 8 }`; the privacy option "trade" is the server's), else `APP_CONFIRM_TRADE` (Yes `TradeReq`, No `ConfirmTradeCancelReq`)
//   ConfirmTradeCancelNtf     the line `TID_GAME_TRADECANCEL`
//   TradeNtf                  the session opens: the inventory and `APP_TRADE`
//   TradePutNtf / TradePullNtf / TradePutGoldNtf   the offers of both sides (an own item is reserved in the bag, an own penya offer is held back
//                             from the displayed wallet)
//   TradeOkNtf / TradeLastConfirm / TradeLastConfirmOk   the OK buttons, `APP_TRADE_CONFIRM` and its wait text, the line `TID_GAME_TRADEACCPET`
//   TradeConsent              the trade is done (the item and penya deltas arrived before): the windows close
//   TradeCancelNtf            the box `TID_DIAG_0003` (cancelled) or `TID_DIAG_0002` (the partner refused) and the windows close
//   TradePutError             the box `TID_DIAG_0054`
//   DelObj of the partner     the windows close (`OnRemoveObj`)
//
// The decisions of the rules stay on the server (D1); the client keeps the legacy gates: the tradeable rule (`RulesFacade.tradeable`, the Rust
// function) before an item is offered, the penya box with `wallet - offered`, and the quantity box of a stack.
//
// Other features reach it with `tradeOf(ctx)`.

import type { ClientContext } from '../../app/context';
import { EntityKind } from '../../game/entities';
import type { InputInstance } from '../../input/input';
import { ITYPE_ITEM } from '../../gen/defs/define';
import {
  APP_CONFIRM_TRADE,
  APP_GUILD_BANK,
  APP_INVENTORY,
  APP_SUMMON_ANGEL,
  APP_TRADE,
  APP_TRADE_GOLD,
} from '../../gen/defs/resdata';
import { MMI_TRADE } from '../../gen/defs/neuz';
import {
  TID_DIAG_0002,
  TID_DIAG_0003,
  TID_DIAG_0054,
  TID_DIAG_0083,
  TID_GAME_FINALCOMFIRM,
  TID_GAME_FROM,
  TID_GAME_MOVECOUNT,
  TID_GAME_MOVEPENYA,
  TID_GAME_NUMCOUNT,
  TID_GAME_PENYACOUNT,
  TID_GAME_TRADEACCPET,
  TID_GAME_TRADECANCEL,
  TID_GAME_TRADEREQUEST,
  TID_GAME_WAITCOMFIRM,
  TID_GAME_YETTRADE,
} from '../../gen/defs/text';
import { SC, type Shortcut } from '../../ui/dnd/shortcut';
import type {
  TradeActions,
  TradeCell,
  TradeSideView,
  TradeView,
  ViewSource,
} from '../../ui/windows/trade/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { formatTid, SYSTEM_CHAT, sysChat } from '../common/text';
import { foreignItemFields, tradeRefusal } from '../common/tradeItems';
import { derivedView } from '../common/view';
import { inventoryOf } from '../inventory';
import { itemFromWire } from '../inventory/items';
import { messengerOf } from '../messenger/api';
import { busyText } from '../npc/busy';
import { menuOf } from '../npc/menuModule';
import { taskbarOf } from '../taskbar/taskbarModule';
import {
  endTrade,
  offeredObjIds,
  onTrade,
  onTradeLastConfirm,
  onTradeLastConfirmOk,
  onTradeOk,
  onTradePull,
  onTradePut,
  onTradePutGold,
} from './reducers';
import { createTradeSenders, type TradeSenders } from './senders';
import {
  defineTradeStore,
  MAX_TRADE,
  type TradeOffer,
  type TradeSide,
  type TradeState,
  type TradeStore,
} from './store';

/** The public face of the trade feature of a context. */
export interface TradeFeature {
  readonly store: TradeStore;
  readonly view: ViewSource<TradeView>;
  readonly actions: TradeActions;
  readonly senders: TradeSenders;
  /** `MMI_TRADE` on a player: the busy checks, then the request. Returns whether `ConfirmTradeReq` was sent. */
  request(targetObjId: number): boolean;
  /** A trade session exists. */
  isActive(): boolean;
  /** Ends the session from this side: `TradeCancelReq{0}` and the windows close (`closeTransactional`). */
  cancel(): void;
}

const features = new WeakMap<ClientContext, TradeFeature>();

/** The trade feature of a context; `undefined` before the module registered. */
export function tradeOf(ctx: ClientContext): TradeFeature | undefined {
  return features.get(ctx);
}

export const tradeModule: FeatureModule = {
  id: 'trade',
  milestone: 4,
  register(fc) {
    registerTrade(fc);
  },
};

const isGoldShortcut = (sc: Shortcut): boolean => sc.kind === SC.ITEM && sc.data === 0;

function registerTrade(fc: FeatureContext): TradeFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const store = defineTradeStore(ctx.stores.registry);
  const senders = createTradeSenders(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number, args: readonly string[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };
  const box = (tid: number): void => {
    fullUi(ctx)?.msgbox.open(text(tid));
  };
  const inv = () => inventoryOf(ctx);
  const stats = { requests: 0, declined: 0, opened: 0, completed: 0, cancelled: 0 };

  const nameOf = (objId: number): string => ctx.entities.get(objId)?.visual?.name ?? '';
  const isMe = (objId: number): boolean => objId === 0 || objId === selfStore.getState().objId;

  // ---- the read model --------------------------------------------------------------------------------------------------------

  const cellCache = new WeakMap<TradeOffer, { version: number; cell: TradeCell }>();
  const cellOf = (key: string, o: TradeOffer): TradeCell => {
    const version = ctx.data?.version ?? -1;
    const hit = cellCache.get(o);
    if (hit?.version === version) return hit.cell;
    const f = foreignItemFields(ctx.data, o.item, key);
    const cell: TradeCell = { ...f, count: o.num, serial: `${f.serial}:${key}` };
    cellCache.set(o, { version, cell });
    return cell;
  };
  const sideView = (name: string, side: TradeSide, key: string): TradeSideView => ({
    name,
    gold: side.gold,
    ok: side.ok,
    cells: side.slots.map((o, i) => (o ? cellOf(`${key}:${String(i)}`, o) : null)),
  });
  const build = (): TradeView => {
    const s = store.getState();
    return {
      version: s.version,
      active: s.active,
      you: sideView(s.partnerName, s.you, 'trade:you'),
      me: sideView(selfStore.getState().name, s.me, 'trade:me'),
    };
  };
  let lastKey = '';
  let lastView: TradeView | null = null;
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? (() => undefined),
  };
  const view = derivedView(
    () => {
      const key = `${String(store.getState().version)}|${selfStore.getState().name}|${String(ctx.data?.version ?? -1)}`;
      if (lastView && key === lastKey) return lastView;
      lastKey = key;
      lastView = build();
      return lastView;
    },
    [store, selfStore, dataSource],
    (a, b) => a === b,
  );

  // ---- windows and reservations ------------------------------------------------------------------------------------------------

  const closeWindows = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    ui.trade.closeFinalConfirm();
    // the penya / count box of the trade goes with it (`SAFE_DELETE( g_WndMng.m_pWndTradeGold )`)
    const qty = ui.wm.get(APP_TRADE_GOLD, 'quantity');
    if (qty) {
      ui.wm.destroy(qty);
      ui.wm.flushDestroyed();
    }
    const w = ui.wm.get(APP_TRADE);
    if (w) ui.wm.destroy(w);
  };

  /** Lifts the reservations of the own offers and the held-back penya, then empties the store. */
  const finish = (): void => {
    const s = store.getState();
    for (const id of offeredObjIds(s)) inv()?.setExtra(id, 0);
    inv()?.setGoldHold(0);
    store.setState(endTrade(s));
  };

  /** The session ended on the other side or by the server: close everything. */
  const sessionOver = (): void => {
    finish();
    closeWindows();
  };

  // an own offer is reserved in the bag and its penya are held back from the shown wallet
  store.subscribe((next, prev) => {
    if (next.me.gold !== prev.me.gold) inv()?.setGoldHold(next.me.gold);
    for (let i = 0; i < MAX_TRADE; i += 1) {
      const a = prev.me.slots[i];
      const b = next.me.slots[i];
      if (a === b) continue;
      if (a && b?.objId !== a.objId) inv()?.setExtra(a.objId, 0);
      if (b) inv()?.setExtra(b.objId, b.num);
    }
  });

  // ---- the actions of the window ------------------------------------------------------------------------------------------

  const cancel = (): void => {
    if (!store.getState().active) return;
    senders.cancel();
  };

  /** A stack asks for the count first (`CWndTradeGold`, "Item", `TID_GAME_MOVECOUNT`), a single item is offered at once. */
  const offerItem = (sc: Shortcut, slot: number): boolean => {
    const i = inv();
    const ui = fullUi(ctx);
    const state = store.getState();
    if (!i || !ui || !state.active || state.final !== 'none') return false;
    // `CWndTradeGold` is open: the drop is swallowed
    if (ui.wm.isOpen(APP_TRADE_GOLD)) return true;
    const it = i.item(sc.id);
    if (!it) return false;
    if (state.me.slots[slot] !== null) return true;
    // `IsFlag( expired )` is a silent refusal; the rest of the rule is the tradeable function of the rules
    const refusal = tradeRefusal(ctx, it, { equipped: i.state().container.isEquip(it.objId) });
    if (refusal !== 0) {
      say(refusal);
      return false;
    }
    if (it.num > 1) {
      ui.msgbox.quantity(
        it.num,
        (n) => {
          // the item may have been reserved or used while the box was open
          const cur = inv()?.item(it.objId);
          if (!cur || cur.extra > 0 || !store.getState().active) return;
          senders.put(slot, cur.objId, Math.min(n, cur.num));
        },
        'Item',
        undefined,
        { prompt: text(TID_GAME_MOVECOUNT), label: text(TID_GAME_NUMCOUNT) },
      );
      return true;
    }
    senders.put(slot, it.objId, 1);
    return true;
  };

  /** The gold coin: the box asks for the penya to ADD (the maximum is the wallet minus what is offered already). */
  const offerGold = (): boolean => {
    const ui = fullUi(ctx);
    const state = store.getState();
    if (!ui || !state.active || state.final !== 'none') return false;
    if (ui.wm.isOpen(APP_TRADE_GOLD)) return true;
    const max = selfStore.getState().gold - state.me.gold;
    if (max < 1) return true;
    ui.msgbox.quantity(
      max,
      (n) => {
        const now = store.getState();
        if (!now.active) return;
        senders.putGold(Math.min(n, selfStore.getState().gold - now.me.gold));
      },
      'Penya',
      undefined,
      { prompt: text(TID_GAME_MOVEPENYA), label: text(TID_GAME_PENYACOUNT) },
    );
    return true;
  };

  const actions: TradeActions = {
    drop(sc, slot, partnerGrid) {
      if (partnerGrid) return isGoldShortcut(sc) ? offerGold() : false;
      if (isGoldShortcut(sc)) return offerGold();
      if (sc.kind !== SC.ITEM || sc.type !== ITYPE_ITEM || slot === null) return false;
      return offerItem(sc, slot);
    },
    pull(slot) {
      if (store.getState().me.slots[slot]) senders.pull(slot);
    },
    ok() {
      if (store.getState().active) senders.ok();
    },
    cancel,
    closed() {
      // `~CWndTrade`: every way of closing the window ends the session (a window that closed after the consent finds none)
      if (!store.getState().active) return;
      senders.cancel();
      sessionOver();
    },
  };

  // ---- the request -------------------------------------------------------------------------------------------------------------

  const request = (targetObjId: number): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    const busy = busyText((app) => ui.wm.isOpen(app));
    if (busy !== null) {
      say(busy);
      return false;
    }
    if (store.getState().active) {
      say(TID_GAME_YETTRADE);
      return false;
    }
    const rec = ctx.entities.get(targetObjId);
    if (rec?.kind !== EntityKind.Player) return false;
    senders.confirmTrade(targetObjId);
    say(TID_GAME_TRADEREQUEST, [rec.visual?.name ?? '']);
    stats.requests += 1;
    return true;
  };

  menuOf(ctx)?.registerAction(MMI_TRADE, ({ targetId, npcKey }) => {
    if (npcKey === null) request(targetId);
  });
  taskbarOf(ctx)?.setTradeRequest(() => {
    // `APP_TRADE` in an applet slot: `SendTrade( focus mover )` (the focused player)
    const focus = (ctx.input as Partial<InputInstance>).core?.targeting.focus ?? null;
    if (focus !== null) request(focus);
  });

  // ---- the messages ------------------------------------------------------------------------------------------------------------

  fc.net.on('confirmTradeNtf', (_m, requester) => {
    const ui = fullUi(ctx);
    if (!ui) return;
    // the auto-decline rule (45 §14.1, `OnConfirmTrade`): the privacy option "trade" is the server's since M6 (PL-095), the busy windows answer with `BlockReq`:
    // 5 a trade or a request window is open, 8 the guild bank or the angel window
    const name = nameOf(requester);
    const busyTrade = store.getState().active || ui.wm.anyOpen([APP_TRADE, APP_CONFIRM_TRADE]);
    const busyOther = ui.wm.anyOpen([APP_GUILD_BANK, APP_SUMMON_ANGEL]);
    if (busyTrade || busyOther) {
      messengerOf(ctx)?.sendBlock(busyTrade ? 5 : 8, name);
      stats.declined += 1;
      return;
    }
    ui.trade.openRequest({
      name,
      fromText: formatTid(fc.texts, TID_GAME_FROM, [name]),
      accept: () => {
        senders.trade(requester);
      },
      decline: () => {
        senders.confirmTradeCancel(requester);
      },
    });
  });

  fc.net.on('confirmTradeCancelNtf', () => {
    say(TID_GAME_TRADECANCEL);
  });

  fc.net.on('tradeNtf', (_m, partnerObj) => {
    const ui = fullUi(ctx);
    // a session that is still open is replaced (a lost cancel): the old one ends first
    if (store.getState().active) finish();
    store.setState(onTrade(store.getState(), partnerObj, nameOf(partnerObj)));
    stats.opened += 1;
    if (!ui) return;
    ui.trade.closeRequest();
    ui.wm.openApplet(APP_INVENTORY);
    ui.trade.openTrade();
  });

  fc.net.on('tradePutNtf', (m, objId) => {
    if (!m.item) return;
    store.setState(
      onTradePut(store.getState(), isMe(objId), m.slot, {
        objId: m.objId,
        num: m.num,
        item: itemFromWire(m.item),
      }),
    );
  });

  fc.net.on('tradePutError', () => {
    box(TID_DIAG_0054);
  });

  fc.net.on('tradePullNtf', (m, objId) => {
    store.setState(onTradePull(store.getState(), isMe(objId), m.slot));
  });

  fc.net.on('tradePutGoldNtf', (m, objId) => {
    store.setState(onTradePutGold(store.getState(), isMe(objId), m.gold));
  });

  fc.net.on('tradeOkNtf', (_m, objId) => {
    const mine = isMe(objId);
    store.setState(onTradeOk(store.getState(), mine));
    if (!mine && store.getState().active) say(TID_GAME_TRADEACCPET, [store.getState().partnerName]);
  });

  fc.net.on('tradeLastConfirm', () => {
    const ui = fullUi(ctx);
    if (!store.getState().active) return;
    store.setState(onTradeLastConfirm(store.getState()));
    ui?.trade.openFinalConfirm({
      question: text(TID_DIAG_0083),
      yes: () => {
        senders.confirm();
      },
      no: () => {
        senders.cancel();
      },
    });
  });

  fc.net.on('tradeLastConfirmOk', (_m, objId) => {
    const s = store.getState();
    if (!s.active) return;
    if (isMe(objId)) {
      store.setState(onTradeLastConfirmOk(s));
      fullUi(ctx)?.trade.finalConfirm()?.wait(text(TID_GAME_WAITCOMFIRM));
    } else {
      // the line has the colour of `TID_GAME_TRADEACCPET`
      fullUi(ctx)?.chat.putString(
        formatTid(fc.texts, TID_GAME_FINALCOMFIRM, [s.partnerName]),
        null,
        fc.texts.textColor(TID_GAME_TRADEACCPET),
        SYSTEM_CHAT,
      );
    }
  });

  fc.net.on('tradeConsent', () => {
    if (!store.getState().active) return;
    stats.completed += 1;
    // the exchange was applied by the item and penya messages before this one (PL-063)
    sessionOver();
  });

  fc.net.on('tradeCancelNtf', (m) => {
    sessionOver();
    stats.cancelled += 1;
    // mode 0: cancelled (also the error form of a failed commit); else the partner refused
    if (m.mode !== 0) {
      if (m.idPlayer !== selfStore.getState().idPlayer) box(TID_DIAG_0002);
    } else {
      box(TID_DIAG_0003);
    }
  });

  fc.net.on('delObj', (_m, objId) => {
    const s = store.getState();
    if (s.active && s.partnerObj === objId) {
      // the partner left the view: the windows close, the server ends the session (`OnRemoveObj`)
      sessionOver();
    }
  });

  // ---- the windows -------------------------------------------------------------------------------------------------------------

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.trade.setPorts({ view, actions });
  };
  ctx.tickSystems.register('trade.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  // every transactional close sends the cancel of a live session
  fc.transactional.register({
    id: 'trade',
    reasons: 'all',
    close: () => {
      if (store.getState().active) senders.cancel();
      sessionOver();
      fullUi(ctx)?.trade.closeRequest();
    },
  });

  const feature: TradeFeature = {
    store,
    view,
    actions,
    senders,
    request,
    isActive: () => store.getState().active,
    cancel,
  };
  features.set(ctx, feature);

  addProbeMember(ctx, 'trade', () => {
    const s: TradeState = store.getState();
    const side = (x: TradeSide) => ({
      gold: x.gold,
      ok: x.ok,
      slots: x.slots.map((o, i) =>
        o ? { slot: i, objId: o.objId, itemId: o.item.itemId, num: o.num } : null,
      ),
    });
    return {
      active: s.active,
      partnerObj: s.partnerObj,
      partnerName: s.partnerName,
      final: s.final,
      me: side(s.me),
      you: side(s.you),
      requestOpen: fullUi(ctx)?.trade.isRequestOpen() ?? false,
      stats: { ...stats },
    };
  });
  return feature;
}

export type { TradeState };
