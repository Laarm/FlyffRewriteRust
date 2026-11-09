// The `pockets` feature (Plan/45 §9.7, Plan/19 §3.5; WP-CFEAT-29, the pockets part): the backpack window `APP_BAG_EX` and every move between the bag and the
// three pockets - the basic pocket and the two extra bags. The server owns the rules (`MoveItemPocket`, `AvailPocket`, the expiry of a bag): this side
// mirrors the contents (`SpawnSelf`, `PocketAddItem`, `PocketRemoveItem`, `PocketAttribute`), asks for the amount of a stack in the quantity box ("Move how
// many items?" / "Number:") and sends the moves.
//
// A bag item dropped on a grid is `MoveItemPocket{-1, objId, n, pocket}` (a worn item says `TID_GAME_EQUIPPUT`, a quest item
// `TID_GAME_ERROR_DONT_MOVE_QUEST_ITEM_TO_BAG_EX`), a pocket item dropped on another grid `MoveItemPocket{from, objId, n, pocket}`, a pocket item dropped on the
// inventory `MoveItemPocket{pocket, objId, n, -1}`; a pocket item (`IK3_POCKET`) dropped on a bag icon is `AvailPocket{pocket, objId}`. The answers change the
// mirror in `pocketsStore` (the bag side of a move is the inventory's `CreateItem` / `UpdateItem`).
//
// Other features reach it with `pocketsOf(ctx)`.

import type { ClientContext } from '../../app/context';
import { APP_BAG_EX, APP_TRADE_GOLD } from '../../gen/defs/resdata';
import { IK3_POCKET, IK3_QUEST } from '../../gen/defs/itemkind';
import {
  TID_GAME_ALREADY_USE,
  TID_GAME_EQUIPPUT,
  TID_GAME_ERROR_DONT_MOVE_QUEST_ITEM_TO_BAG_EX,
  TID_GAME_MOVECOUNT,
  TID_GAME_NUMCOUNT,
} from '../../gen/defs/text';
import { wallNow } from '../../platform/time';
import { SC, type Shortcut } from '../../ui/dnd/shortcut';
import type { PocketActions, PocketView } from '../../ui/windows/pockets/ports';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { derivedView, type ViewSource } from '../common/view';
import { itemFromWire } from '../inventory/items';
import { inventoryOf } from '../inventory/window/inventoryModule';
import { createPocketSenders, POCKET_BAG, type PocketSenders } from './senders';
import {
  applyAdd,
  applyAttribute,
  applyRemove,
  applySelf,
  definePocketsStore,
  pocketAvailable,
  pocketCapacity,
  POCKET_COUNT,
  type PocketsState,
  type PocketsStore,
} from './state';
import { PocketViewBuilder } from './view';

/** The public face of the pockets feature of a context. */
export interface PocketsFeature {
  readonly store: PocketsStore;
  readonly view: ViewSource<PocketView>;
  readonly actions: PocketActions;
  readonly senders: PocketSenders;
  state(): PocketsState;
  /** `IsAvailable( pocket )`. */
  available(pocket: number): boolean;
  /** Opens (or focuses) the window; `false` when it refused to exist (a trade, a shop, ... is open). */
  open(): boolean;
  isOpen(): boolean;
}

const features = new WeakMap<ClientContext, PocketsFeature>();

/** The pockets feature of a context; `undefined` before the module registered. */
export function pocketsOf(ctx: ClientContext): PocketsFeature | undefined {
  return features.get(ctx);
}

/** The worn item of a drag out of the inventory comes with `index = 0xffffffff` and no selection (`SelectCount != 1`). */
const WORN = 0xffff_ffff;

export const pocketsModule: FeatureModule = {
  id: 'pockets',
  milestone: 4,
  register(fc) {
    const ctx = fc.app;
    const store = definePocketsStore(ctx.stores.registry);
    const senders = createPocketSenders(ctx);
    const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
    const say = (tid: number, args: readonly string[] = []): void => {
      sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
    };
    const stats = { moves: 0, avails: 0, refused: 0 };
    const nowSec = (): number => Math.floor(wallNow() / 1000);
    const perm = (): boolean => ctx.features.features.perm_extrabags;
    const capacityOf = (i: number): number =>
      pocketCapacity(i, ctx.features.features.bigger_pocket);

    // ---- the view ---------------------------------------------------------------------------------------------------------

    const builder = new PocketViewBuilder({
      data: () => ctx.data,
      permExtrabags: perm,
      newFontColor: () => ctx.features.features.new_font_color,
      nowSec,
      text,
    });
    const dataSource = {
      subscribe: (fn: () => void) =>
        ctx.data?.onBundle(() => {
          fn();
        }) ?? (() => undefined),
    };
    const view = derivedView(
      () => builder.build(store.getState()),
      [store, dataSource],
      (a, b) => a === b,
    );

    // ---- helpers ----------------------------------------------------------------------------------------------------------

    const inv = () => inventoryOf(ctx);
    const prop = (id: number) => ctx.data?.item(id);
    const available = (pocket: number): boolean => builder.available(store.getState(), pocket);

    /** `SAFE_DELETE( g_WndMng.m_pWndTradeGold )`. */
    const closeQuantity = (): void => {
      fullUi(ctx)?.wm.get(APP_TRADE_GOLD, 'quantity')?.destroy();
    };

    /** The quantity box for a stack (`TID_GAME_MOVECOUNT` / `TID_GAME_NUMCOUNT`). */
    const askCount = (max: number, then: (n: number) => void): void => {
      fullUi(ctx)?.msgbox.quantity(max, then, undefined, undefined, {
        prompt: text(TID_GAME_MOVECOUNT),
        label: text(TID_GAME_NUMCOUNT),
      });
    };

    /** A stack asks for the amount, a single piece goes at once. */
    const moveOrAsk = (from: number, objId: number, num: number, to: number): void => {
      if (num > 1) {
        askCount(num, (n) => {
          stats.moves += 1;
          senders.move(from, objId, n, to);
        });
      } else {
        stats.moves += 1;
        senders.move(from, objId, 1, to);
      }
    };

    // ---- moves ------------------------------------------------------------------------------------------------------------

    const actions: PocketActions = {
      moveIn(sc, pocket) {
        if (sc.kind !== SC.ITEM || pocket < 0 || pocket >= POCKET_COUNT) return false;
        closeQuantity();
        // the coin of the inventory does nothing here
        if (sc.data === 0) return false;
        if (sc.index === WORN) {
          say(TID_GAME_EQUIPPUT);
          return false;
        }
        const item = inv()?.item(sc.id) ?? null;
        if (!item) return false;
        if (prop(item.itemId)?.item_kind3 === IK3_QUEST) {
          say(TID_GAME_ERROR_DONT_MOVE_QUEST_ITEM_TO_BAG_EX);
          stats.refused += 1;
          return false;
        }
        // a stack of which a window holds a part (`GetExtra() != 0`) opens no amount box and sends nothing
        if (item.num > 1 && item.extra !== 0) return true;
        moveOrAsk(POCKET_BAG, item.objId, item.num, pocket);
        return true;
      },
      moveBetween(sc, pocket) {
        const from = sc.userId;
        if (sc.kind !== SC.ITEM || pocket < 0 || pocket >= POCKET_COUNT) return false;
        if (from < 0 || from >= POCKET_COUNT || from === pocket) return false;
        closeQuantity();
        if (sc.data === 0) return false;
        const item = store.getState().pockets[from]?.container.getAtId(sc.id) ?? null;
        if (!item) return false;
        moveOrAsk(from, item.objId, item.num, pocket);
        return true;
      },
      availBag(sc, pocket) {
        if (sc.kind !== SC.ITEM || pocket < 1 || pocket >= POCKET_COUNT) return false;
        const item = inv()?.item(sc.id) ?? null;
        if (!item) return false;
        if (prop(item.itemId)?.item_kind3 !== IK3_POCKET) return false;
        if (available(pocket)) {
          // the bag is in use already
          fullUi(ctx)?.msgbox.open(text(TID_GAME_ALREADY_USE));
          return false;
        }
        stats.avails += 1;
        senders.avail(pocket, item.objId);
        return true;
      },
      closed() {
        closeQuantity();
      },
    };

    /** A pocket item dropped on the bag (`CWndInventory::OnChildNotify`, the `APP_BAG_EX` branch): `MoveItemPocket{pocket, objId, n, -1}`. */
    const dropIn = (sc: Shortcut): boolean => {
      if (sc.kind !== SC.ITEM) return false;
      const from = sc.userId;
      closeQuantity();
      if (from < 0 || from >= POCKET_COUNT) return false;
      // the shortcut of the coin carries no item: nothing to do (and no forbid icon: `bForbid = FALSE`)
      if (sc.data === 0) return true;
      if (!available(from)) return false;
      const item = store.getState().pockets[from]?.container.getAtId(sc.id) ?? null;
      if (!item) return false;
      moveOrAsk(from, item.objId, item.num, POCKET_BAG);
      return true;
    };

    let unregisterDrop: (() => void) | null = null;
    const hookInventory = (): void => {
      const i = inv();
      if (!i || unregisterDrop) return;
      unregisterDrop = i.registerDropIn(APP_BAG_EX, dropIn);
    };
    ctx.tickSystems.register('pockets.inventory', hookInventory);
    hookInventory();

    // ---- the handlers -----------------------------------------------------------------------------------------------------

    fc.net.on('spawnSelf', (m) => {
      store.setState(applySelf(store.getState(), m.self?.pockets, capacityOf));
    });
    fc.net.on('pocketAttribute', (m) => {
      store.setState(
        applyAttribute(store.getState(), m.attribute, m.pocket, m.data, nowSec(), capacityOf),
      );
    });
    fc.net.on('pocketAddItem', (m) => {
      if (!m.item) return;
      const item = itemFromWire(m.item);
      store.setState(
        applyAdd(
          store.getState(),
          m.pocket,
          item,
          prop(item.itemId)?.pack_max ?? 1,
          available(m.pocket),
        ),
      );
    });
    fc.net.on('pocketRemoveItem', (m) => {
      store.setState(applyRemove(store.getState(), m.pocket, m.objId, m.num, available(m.pocket)));
    });

    const api: PocketsFeature = {
      store,
      view,
      actions,
      senders,
      state: () => store.getState(),
      available,
      open: () => {
        const ui = fullUi(ctx);
        if (!ui) return false;
        return ui.pockets.open() !== null;
      },
      isOpen: () => fullUi(ctx)?.wm.isOpen(APP_BAG_EX) ?? false,
    };
    features.set(ctx, api);

    // the window shell reads the view and the actions of this feature
    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.pockets.setPorts({ view, actions });
    };
    ctx.tickSystems.register('pockets.ui', ensureUi);
    ctx.machine.subscribe(ensureUi);
    ensureUi();

    addProbeMember(ctx, 'pockets', () => {
      const s = store.getState();
      return {
        open: api.isOpen(),
        ready: s.ready,
        permanent: perm(),
        pockets: s.pockets.map((p, i) => ({
          index: i,
          available: pocketAvailable(perm(), i, p.exists, p.expired),
          exists: p.exists,
          expired: p.expired,
          expireAt: p.expireAt,
          capacity: p.container.indexNum,
          items: Array.from({ length: p.container.indexNum }, (_, slot) =>
            p.container.getAtSlot(slot),
          )
            .filter((it) => it !== null)
            .map((it) => ({ objId: it.objId, itemId: it.itemId, num: it.num })),
        })),
        stats: { ...stats },
      };
    });
  },
};
