// The `bank` feature (Plan/45 §13, Plan/19 §10; WP-CFEAT-20): the PIN flow, the bank window and every move between the bag and the three
// compartments. The flow is the server's: `OpenBankWnd` is answered with `BankWindow` (mode 0 = the PIN is still "0000": ask for a new one,
// else ask for the PIN), `ConfirmBank` with `ConfirmBankPass` (ok: the inventory and the bank open; not ok: the entry again and "Wrong password"),
// `ChangeBankPassReq` with `ChangeBankPassNtf` (ok: the entry; not ok: the change window again and "Wrong password"). The bank window is
// destroyed -> `CloseBankWnd` (the session ends); so is a PIN window that the PLAYER closed. A window that the flow replaced sends nothing.
//
// Moves (Plan/45 §13.2): an inventory item or coin dropped on a compartment, a compartment item or coin dropped on the inventory or on
// another compartment. A stack asks for the amount in the quantity box ("Move how many items?" / "Number:", the penya "How many do you want to
// move?" / "Penya:"); the sends are `PutItemBack`, `GetItemBack`, `PutGoldBack`, `GetGoldBack` and `PutBackToBank`. The answers change the
// mirror in `bankStore` (the bag side of a withdrawal is the inventory's `CreateItem`). Remote-bank compartments of other characters are
// usable only while `SM_BANK15` / `SM_BANK30` / `SM_BANK001` is active.
//
// Other features reach it with `bankOf(ctx)`.

import type { ClientContext } from '../../app/context';
import { APP_COMMON_BANK, APP_INVENTORY, APP_TRADE_GOLD } from '../../gen/defs/resdata';
import { MMI_BANKING } from '../../gen/defs/neuz';
import {
  TID_DIAG_0028,
  TID_GAME_EQUIPPUT,
  TID_GAME_MOVECOUNT,
  TID_GAME_MOVEPENYA,
  TID_GAME_NUMCOUNT,
  TID_GAME_PENYACOUNT,
  TID_GAME_SLOTFULL,
  TID_GMAE_CHAOTIC_NOT_BANK,
} from '../../gen/defs/text';
import { wallNow } from '../../platform/time';
import { SC, type Shortcut } from '../../ui/dnd/shortcut';
import type {
  BankActions,
  BankPasswordRequest,
  BankView,
  ConfirmBankRequest,
} from '../../ui/windows/bank/ports';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { derivedView, type ViewSource } from '../common/view';
import { SM } from '../buffs/sm';
import { itemFromWire } from '../inventory/items';
import { inventoryOf } from '../inventory/window/inventoryModule';
import { menuOf } from '../npc/menuModule';
import { bankBusy } from './busy';
import { BANK_NULL_ID, createBankSenders, type BankSenders } from './senders';
import {
  applyPutGold,
  applyPutItem,
  applySelf,
  applyUpdateItem,
  BANK_SLOTS,
  defineBankStore,
  type BankState,
  type BankStore,
} from './state';
import { BankViewBuilder } from './view';

/** Where the PIN flow stands (the probe and the tests read it). */
export type BankPhase =
  /** No bank window. */
  | 'closed'
  /** The PIN entry (`APP_CONFIRM_BANK`) is up. */
  | 'confirm'
  /** The first PIN window (`APP_BANK_PASSWORD`, "0000" in the old field). */
  | 'setup'
  /** The PIN change window. */
  | 'change'
  /** The bank is open. */
  | 'open';

/** `INT_MAX`: what the quantity box of the original clamps the penya to. */
const INT_MAX = 0x7fff_ffff;

/** The public face of the bank feature of a context. */
export interface BankFeature {
  readonly store: BankStore;
  readonly view: ViewSource<BankView>;
  readonly actions: BankActions;
  readonly senders: BankSenders;
  state(): BankState;
  phase(): BankPhase;
  /** `MMI_BANKING` on a banker: the busy checks, the chaotic penalty, then `OpenBankWnd(NULL_ID, 0)`. */
  requestOpen(): boolean;
  /** Closes every bank window (a transactional close: the session ends). */
  close(): void;
  /** The bank window is open. */
  isOpen(): boolean;
}

const features = new WeakMap<ClientContext, BankFeature>();

/** The bank feature of a context; `undefined` before the module registered. */
export function bankOf(ctx: ClientContext): BankFeature | undefined {
  return features.get(ctx);
}

export const bankModule: FeatureModule = {
  id: 'bank',
  milestone: 4,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const store = defineBankStore(ctx.stores.registry);
    const senders = createBankSenders(ctx);
    const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
    const say = (tid: number, args: readonly string[] = []): void => {
      sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
    };
    const stats = { opened: 0, confirmed: 0, wrongPin: 0, moves: 0, closes: 0 };
    let phase: BankPhase = 'closed';
    let ids = { objId: BANK_NULL_ID, itemId: 0 };

    // ---- the view ---------------------------------------------------------------------------------------------------------

    const builder = new BankViewBuilder({
      data: () => ctx.data,
      commBank: () => {
        const now = Math.floor(wallNow() / 1000);
        return selfStore
          .getState()
          .smTimers.some(
            (t) =>
              (t.kind === SM.BANK15 || t.kind === SM.BANK30 || t.kind === SM.BANK001) &&
              t.endUnix > now,
          );
      },
      newFontColor: () => ctx.features.features.new_font_color,
    });
    const dataSource = {
      subscribe: (fn: () => void) =>
        ctx.data?.onBundle(() => {
          fn();
        }) ?? (() => undefined),
    };
    const view = derivedView(
      () => builder.build(store.getState()),
      [store, selfStore, dataSource],
      (a, b) => a === b,
    );

    // ---- helpers ----------------------------------------------------------------------------------------------------------

    const inv = () => inventoryOf(ctx);
    const prop = (id: number) => ctx.data?.item(id);

    const closeQuantity = (): void => {
      fullUi(ctx)?.wm.get(APP_TRADE_GOLD, 'quantity')?.destroy();
    };

    /** The quantity box for a stack of items (`TID_GAME_MOVECOUNT` / `TID_GAME_NUMCOUNT`). */
    const askCount = (max: number, then: (n: number) => void): void => {
      fullUi(ctx)?.msgbox.quantity(max, then, undefined, undefined, {
        prompt: text(TID_GAME_MOVECOUNT),
        label: text(TID_GAME_NUMCOUNT),
      });
    };

    /** The quantity box for penya (`TID_GAME_MOVEPENYA` / `TID_GAME_PENYACOUNT`), the maximum clamped to `INT_MAX`. */
    const askPenya = (max: number, then: (n: number) => void): void => {
      fullUi(ctx)?.msgbox.quantity(Math.min(max, INT_MAX), then, undefined, undefined, {
        prompt: text(TID_GAME_MOVEPENYA),
        label: text(TID_GAME_PENYACOUNT),
      });
    };

    // ---- the PIN windows --------------------------------------------------------------------------------------------------

    const endSession = (): void => {
      if (phase === 'closed') return;
      phase = 'closed';
      closeQuantity();
      senders.closeBank();
      stats.closes += 1;
    };

    const confirmRequest = (): ConfirmBankRequest => {
      const mine = ids;
      return {
        confirm: (pin) => {
          senders.confirmBank(pin, mine.objId, mine.itemId);
        },
        changePassword: () => {
          openPassword(false);
        },
        closed: (how) => {
          if (how === 'user') endSession();
        },
      };
    };

    const passwordRequest = (initial: boolean): BankPasswordRequest => {
      const mine = ids;
      return {
        initial,
        change: (oldPin, newPin) => {
          senders.changePass(oldPin, newPin, mine.objId, mine.itemId);
        },
        closed: (how) => {
          if (how === 'user') endSession();
        },
      };
    };

    function openConfirm(): void {
      phase = 'confirm';
      fullUi(ctx)?.bank.openConfirm(confirmRequest());
    }
    function openPassword(initial: boolean): void {
      phase = initial ? 'setup' : 'change';
      fullUi(ctx)?.bank.openPassword(passwordRequest(initial));
    }
    const wrongPassword = (): void => {
      stats.wrongPin += 1;
      fullUi(ctx)?.msgbox.open(text(TID_DIAG_0028));
    };

    // ---- moves ------------------------------------------------------------------------------------------------------------

    const walletGold = (): number => Math.max(0, selfStore.getState().gold);

    const actions: BankActions = {
      putFromInventory(sc, slot) {
        if (sc.kind !== SC.ITEM || slot < 0 || slot >= BANK_SLOTS) return false;
        if (sc.data !== 0) {
          // an item: a worn one comes with `index = 0xffffffff` and no selection (`SelectCount != 1`)
          if (sc.index === 0xffff_ffff) {
            say(TID_GAME_EQUIPPUT);
            return false;
          }
          const item = inv()?.item(sc.id) ?? null;
          if (!item) return false;
          const objId = item.objId;
          if (item.num > 1) {
            askCount(item.num, (n) => {
              stats.moves += 1;
              senders.putItem(slot, objId, n);
            });
          } else {
            stats.moves += 1;
            senders.putItem(slot, objId, 1);
          }
          return true;
        }
        // the coin of the inventory: the amount is asked, the wallet is the maximum
        askPenya(walletGold(), (n) => {
          const gold = Math.min(n, walletGold());
          if (gold <= 0) return;
          stats.moves += 1;
          senders.putGold(slot, gold);
        });
        return true;
      },
      moveBetween(sc, slot) {
        const from = sc.userId;
        if (sc.kind !== SC.ITEM || slot < 0 || slot >= BANK_SLOTS) return false;
        if (from < 0 || from >= BANK_SLOTS || from === slot) return false;
        const src = store.getState().compartments[from];
        if (!src) return false;
        if (sc.data !== 0) {
          const item = src.container.getAtId(sc.id);
          if (!item) return false;
          if (item.num > 1) {
            askCount(item.num, (n) => {
              stats.moves += 1;
              senders.moveItem(from, slot, item.objId, n);
            });
          } else {
            stats.moves += 1;
            senders.moveItem(from, slot, item.objId, 1);
          }
          return true;
        }
        askPenya(src.gold, (n) => {
          const gold = Math.min(n, store.getState().compartments[from]?.gold ?? 0);
          if (gold <= 0) return;
          stats.moves += 1;
          senders.moveGold(from, slot, gold);
        });
        return true;
      },
      closed() {
        endSession();
      },
    };

    /** A compartment item or coin dropped on the bag (`CWndInventory::OnChildNotify`, the `APP_COMMON_BANK` branch). */
    const dropIn = (sc: Shortcut): boolean => {
      if (sc.kind !== SC.ITEM) return false;
      const from = sc.userId;
      const comp = store.getState().compartments[from];
      if (!comp) return false;
      if (sc.data !== 0) {
        const item = comp.container.getAtId(sc.id);
        if (!item) return false;
        if (item.num > 1) {
          askCount(item.num, (n) => {
            stats.moves += 1;
            senders.getItem(from, item.objId, n);
          });
        } else {
          stats.moves += 1;
          senders.getItem(from, item.objId, 1);
        }
        return true;
      }
      askPenya(comp.gold, (n) => {
        const gold = Math.min(n, store.getState().compartments[from]?.gold ?? 0);
        if (gold <= 0) return;
        stats.moves += 1;
        senders.getGold(from, gold);
      });
      return true;
    };

    let unregisterDrop: (() => void) | null = null;
    const hookInventory = (): void => {
      const i = inv();
      if (!i || unregisterDrop) return;
      unregisterDrop = i.registerDropIn(APP_COMMON_BANK, dropIn);
    };
    ctx.tickSystems.register('bank.inventory', hookInventory);
    hookInventory();

    // ---- opening -----------------------------------------------------------------------------------------------------------

    /** `MMI_BANKING` (`CWndWorld::OnCommand`): the busy checks in the order of the original, the chaotic penalty, then the request. */
    const requestOpen = (): boolean => {
      const ui = fullUi(ctx);
      if (!ui) return false;
      const busy = bankBusy((app) => ui.wm.isOpen(app));
      if (busy === 'silent') return false;
      if (busy !== null) {
        say(busy);
        return false;
      }
      const pk = selfStore.getState().pk;
      if (pk.propensity > 0) {
        const rows = ctx.data?.bundle('tables')?.pk.penalties ?? [];
        const row =
          rows.find((r) => r.min <= pk.propensity && pk.propensity <= r.max) ??
          rows.find((r) => r.step === 8);
        // without the penalty table the server decides (it checks the same row)
        if (rows.length > 0 && (!row || row.bank === 0)) {
          say(TID_GMAE_CHAOTIC_NOT_BANK);
          return false;
        }
      }
      stats.opened += 1;
      senders.openBank(BANK_NULL_ID, 0);
      return true;
    };

    const menu = menuOf(ctx);
    if (!menu)
      throw new Error('the bank feature needs the npc menu table (register the npc module first)');
    menu.registerAction(MMI_BANKING, ({ npcKey }) => {
      if (npcKey !== null) requestOpen();
    });

    // ---- the handlers -----------------------------------------------------------------------------------------------------

    fc.net.on('spawnSelf', (m) => {
      store.setState(applySelf(store.getState(), m.self));
    });
    fc.net.on('bankWindow', (m) => {
      const ui = fullUi(ctx);
      ids = { objId: m.objId, itemId: m.itemId };
      ui?.bank.closeAll('replaced');
      if (m.mode !== 0) openConfirm();
      else openPassword(true);
    });
    fc.net.on('confirmBankPass', (m) => {
      const ui = fullUi(ctx);
      ids = { objId: m.objId, itemId: m.itemId };
      ui?.bank.closeAll('replaced');
      if (m.ok) {
        stats.confirmed += 1;
        phase = 'open';
        // `CreateApplet( APP_INVENTORY )`, then the bank beside it
        ui?.char.openInventory();
        ui?.bank.openBank({ view, actions });
      } else {
        openConfirm();
        wrongPassword();
      }
    });
    fc.net.on('changeBankPassNtf', (m) => {
      const ui = fullUi(ctx);
      ids = { objId: m.objId, itemId: m.itemId };
      ui?.bank.closeAll('replaced');
      if (m.ok) {
        openConfirm();
      } else {
        openPassword(false);
        wrongPassword();
      }
    });
    fc.net.on('putItemBank', (m) => {
      if (!m.item) return;
      const item = itemFromWire(m.item);
      store.setState(
        applyPutItem(store.getState(), m.slot, item, prop(item.itemId)?.pack_max ?? 1),
      );
    });
    fc.net.on('updateBankItem', (m) => {
      store.setState(applyUpdateItem(store.getState(), m.slot, m.objId, m.param, m.value));
    });
    fc.net.on('putGoldBank', (m) => {
      store.setState(applyPutGold(store.getState(), m.slot, m.bankGold));
      // `g_pPlayer->SetGold( dwGold )`: the wallet after the move
      selfStore.setState({ gold: m.gold });
    });
    fc.net.on('bankIsFull', () => {
      say(TID_GAME_SLOTFULL);
    });
    // `GetItemBank` is informational (the bag side is the `CreateItem` that comes with it); `MoveBankItem` is never sent.

    // ---- transactional close ------------------------------------------------------------------------------------------------

    fc.transactional.register({
      id: 'bank',
      reasons: 'all',
      close: () => {
        fullUi(ctx)?.bank.closeAll('user');
        endSession();
      },
    });

    const api: BankFeature = {
      store,
      view,
      actions,
      senders,
      state: () => store.getState(),
      phase: () => phase,
      requestOpen,
      close: () => {
        fullUi(ctx)?.bank.closeAll('user');
        endSession();
      },
      isOpen: () => fullUi(ctx)?.wm.isOpen(APP_COMMON_BANK) ?? false,
    };
    features.set(ctx, api);

    addProbeMember(ctx, 'bank', () => {
      const s = store.getState();
      const v = view.get();
      return {
        phase,
        open: api.isOpen(),
        inventoryOpen: fullUi(ctx)?.wm.isOpen(APP_INVENTORY) ?? false,
        ids: { ...ids },
        slot: s.slot,
        gold: selfStore.getState().gold,
        compartments: s.compartments.map((c, i) => ({
          index: i,
          characterId: c.characterId,
          gold: c.gold,
          usable: v.compartments[i]?.usable ?? false,
          items: Array.from({ length: c.container.indexNum }, (_, slot) =>
            c.container.getAtSlot(slot),
          )
            .filter((it) => it !== null)
            .map((it) => ({ objId: it.objId, itemId: it.itemId, num: it.num })),
        })),
        stats: { ...stats },
      };
    });
  },
};
