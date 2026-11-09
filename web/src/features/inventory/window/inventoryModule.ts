// The `inventory` feature module (Plan/45 §9; WP-CFEAT-10): the own bag and equipment from the messages of the item protocol
// (`SpawnSelf.inventory`, `CreateItem`, `MoveItemNtf`, `UpdateItem(Ex)`, `DoEquipNtf`, `HideCoatNtf`), the ports of the inventory
// window of 42 (`ui.char.setPorts({inventory})`), the item source of the taskbar and the ITEM shortcut port, the world-drop
// handler of the drag controller (drop to the ground) and the numbers provider of the item tooltips.
//
// Other features reach it with `inventoryOf(ctx)`: reads (`item`, `find`, `countOf`, `equipped`), the reservation of a stack
// (`setExtra`), the dispatch seams of WP-CFEAT-13/14 (`use.hooks`) and the dialog factory of the drop flows.

import type { ClientContext } from '../../../app/context';
import { inputProvidersOf } from '../../../app/inputProviders';
import { PARTS_BULLET } from '../../../gen/defs/neuz';
import { IK3_ARROW } from '../../../gen/defs/itemkind';
import { EntityKind } from '../../../game/entities';
import { peekRulesRuntime } from '../../../game/rules';
import { wallNow } from '../../../platform/time';
import type { InventoryView } from '../../../ui/windows/char/ports';
import type { Shortcut } from '../../../ui/dnd/shortcut';
import type { ViewSource } from '../../common/view';
import { derivedView } from '../../common/view';
import { addProbeMember } from '../../common/probe';
import { itemIconPath } from '../../common/itemIcon';
import { sysChat } from '../../common/text';
import { type FeatureModule, fullUi } from '../../common/module';
import { setOptionByPath } from '../../options/store';
import { defineOptionsStore } from '../../options';
import type { TaskbarItemSource } from '../../taskbar/taskbarModule';
import { taskbarOf } from '../../taskbar/taskbarModule';
import { type DropEnv, dropOnWorld } from '../drop/drop';
import { partsOf } from '../info';
import { itemFromWire, type InvItem, UI } from '../items';
import { itemRulesOf } from '../rules';
import { createInventorySenders, type InventorySenders } from '../senders';
import {
  applyAddItem,
  applyContainer,
  applyCooldown,
  applyCreateItem,
  applyEquip,
  applyExtra,
  applyHideCoat,
  applyMoveItem,
  applyUpdateItem,
  applyUpdateItemEx,
  cooldownGroup,
  defineInventoryStore,
  type InventoryState,
  type InventoryStore,
} from '../state';
import { type InvItemView, InventoryViewBuilder } from '../view';
import { createInventoryActions } from './actions';
import { groundPosition } from './ground';
import type { ItemUse } from './use';
import { createDispatchGlue } from './dispatchEnv';
import { createShiftTracker, createSummonAngelRequest } from '../dispatch/windows';
import { MOT_BASE_ESCAPE } from '../../../gen/defs/neuz';
import { WI_WORLD_GUILDWAR } from '../../../gen/defs/world';
import { TID_GAME_GUILDCOMBAT_NO_OUTWAR } from '../../../gen/defs/text';
import { motionsOf } from '../../motions';
import { APP_INVENTORY, APP_SUMMON_ANGEL } from '../../../gen/defs/resdata';
import type { DispatchEnv, DispatchWindows } from '../dispatch/env';
import type { ItemUseSenders } from '../dispatch/senders';
import { applyOwnVisual, setVisualFlag, setVisualPart } from './visual';
import type { InventoryActions } from '../../../ui/windows/char/ports';
import { IK2_FOOD, IK2_SKILL, IK3_PILL } from '../../../gen/defs/itemkind';

/** The public face of the inventory feature of a context. */
export interface InventoryFeature {
  readonly store: InventoryStore;
  readonly view: ViewSource<InventoryView>;
  readonly actions: InventoryActions;
  /** The double-click dispatch, the shortcut port and the router (hooks for WP-CFEAT-13/14). */
  readonly use: ItemUse;
  readonly senders: InventorySenders;
  /** The sends of the item dispatch (the buff icons' double click reuses them). */
  readonly itemSenders: ItemUseSenders;
  /** Opens the angel summoning window over the inventory (`APP_SUMMON_ANGEL`; the NPC dialogs of the event call it). */
  openSummonAngel(): void;
  /** The taskbar's view of the inventory (installed with `taskbarOf(ctx).setItems`). */
  readonly taskbarSource: TaskbarItemSource;
  state(): InventoryState;
  /** `GetItemId(objId)`. */
  item(objId: number): InvItem | null;
  /** `GetEquip(part)`. */
  equipped(part: number): InvItem | null;
  /** `GetItemCount(itemId)`: the total over the bag and the equipment. */
  countOf(itemId: number): number;
  /** `Find(itemId)`: the first stack in the bag. */
  find(itemId: number): InvItem | null;
  /** An arrow is in the bullet slot (`IsBullet`). */
  bulletEquipped(): boolean;
  /** `SetExtra`: reserves part of a stack for a client window (dims it, forbids dragging it). */
  setExtra(objId: number, extra: number): void;
  /** `m_Inventory.Add( &item )`: a silent client-side add (the guild bank's `GetItemGuildBankNtf{item_mine}`: the server sends no `CreateItem`). Returns whether it fitted. */
  addItem(item: InvItem): boolean;
  /**
   * The penya a trade has offered (`AddGold( -gold )` of `OnTradePutGold`): the server keeps them in the wallet as a reservation, the inventory
   * window shows `wallet - hold` while the trade lasts (Plan/45 §14.2). `0` lifts it.
   */
  setGoldHold(hold: number): void;
  /** The shell-facing view of an item (name, icon, record). */
  itemView(item: InvItem): InvItemView | null;
  /** `g_eLocal.GetState(EVE_DROPITEMREMOVE)`: replaceable by the event store. */
  setDropRemoves(fn: () => boolean): void;
  /**
   * The handler of a shortcut dropped on the bag grid that came from window `fromApp` (the shop buys, the vendor and the bank
   * move items: 45 §9.4). The handler answers whether the drop was accepted; a refusal shows the forbid icon.
   */
  registerDropIn(fromApp: number, fn: (sc: Shortcut) => boolean): () => void;
  /**
   * Adds windows of later work packages to the item dispatch (`DispatchWindows`: the class-change windows of WP-CFEAT-33, ...): a member
   * that is already there is replaced. Until a window is registered the dispatch refuses its item with `TID_SBEVE_NOTUSEITEM`.
   */
  registerDispatchWindows(extra: Partial<DispatchWindows>): void;
  /**
   * Adds facts of the client that the dispatch asks and the inventory cannot know (`hasPet`: the summoned system pet exists, WP-CFEAT-27; the ticket
   * tables); a member that is already there is replaced.
   */
  registerDispatchEnv(extra: Partial<Pick<DispatchEnv, 'hasPet' | 'ticketOf'>>): void;
  /**
   * The window-feeding protocol (rule D3): the double click of an item while window `app` is open hands the item to it; the handler
   * answers whether the window took it.
   */
  registerFeed(app: number, fn: (item: InvItem) => boolean): () => void;
}

const features = new WeakMap<ClientContext, InventoryFeature>();

/** The inventory feature of a context; `undefined` before the module registered. */
export function inventoryOf(ctx: ClientContext): InventoryFeature | undefined {
  return features.get(ctx);
}

export const inventoryModule: FeatureModule = {
  id: 'inventory',
  milestone: 3,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const store = defineInventoryStore(ctx.stores.registry);
    const options = defineOptionsStore(ctx.stores.registry);
    const nowSec = (): number => Math.floor(wallNow() / 1000);
    const rules = itemRulesOf(() => ctx.data, nowSec);
    const senders = createInventorySenders(ctx);
    const prop = (id: number) => ctx.data?.item(id);
    const say = (tid: number): void => {
      sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
    };
    let dropRemoves: () => boolean = () => true;
    let goldHold = 0;
    const holdListeners = new Set<() => void>();
    const holdSource = {
      subscribe: (fn: () => void) => {
        holdListeners.add(fn);
        return () => {
          holdListeners.delete(fn);
        };
      },
    };
    const dropIns = new Map<number, (sc: Shortcut) => boolean>();
    const feeds = new Map<number, (item: InvItem) => boolean>();

    // ---- the view ------------------------------------------------------------------------------------------------------

    const builder = new InventoryViewBuilder({
      data: () => ctx.data,
      features: () => ({
        sorting: ctx.features.features.inventory_sorting,
        hideFashion: ctx.features.features.hide_fashion,
        newFontColor: ctx.features.features.new_font_color,
      }),
      gold: () => selfStore.getState().gold - goldHold,
      modeFlags: () => selfStore.getState().modeFlags,
      subject: () => ctx.entities.self()?.visual,
    });
    const dataSource = {
      subscribe: (fn: () => void) =>
        ctx.data?.onBundle(() => {
          fn();
        }) ?? (() => undefined),
    };
    const view = derivedView(
      () => builder.build(store.getState()),
      [store, selfStore, dataSource, holdSource],
      (a, b) => a === b,
    );

    // ---- item flows ---------------------------------------------------------------------------------------------------------

    const shiftDown = createShiftTracker();
    const summonRequest = createSummonAngelRequest({
      item: (id) => store.getState().container.getAtId(id),
      prop,
      setExtra: (objId, extra) => {
        store.setState(applyExtra(store.getState(), objId, extra));
      },
      shiftDown,
      message: (tid) => {
        say(tid);
      },
      inventoryRect: () => {
        const w = fullUi(ctx)?.wm.get(APP_INVENTORY);
        return w ? { ...w.rectWindow } : null;
      },
      create: (materials) => {
        glue.senders.createAngel(materials);
      },
    });
    const glue = createDispatchGlue({
      fc,
      state: () => store.getState(),
      baseSenders: senders,
      extraWindows: {
        // D3: the open summon window takes an orichalcum or moonstone stack
        feed: (app, item) => {
          if (app !== APP_SUMMON_ANGEL) return feeds.get(app)?.(item) ?? false;
          const win = fullUi(ctx)?.itemUse.summonAngel();
          const stack = summonRequest.stack(item.objId);
          if (!win || !stack) return false;
          win.setDie(stack);
          return true;
        },
      },
    });
    const use = glue.dispatch;

    const itemView = (it: InvItem): InvItemView | null => {
      const v = builder.build(store.getState());
      const c = store.getState().container;
      const slot = c.slotOf(it.objId);
      if (slot === null) return null;
      const found = slot < c.indexNum ? v.bag[slot] : v.equip[slot - c.indexNum];
      return (found as InvItemView | null | undefined) ?? null;
    };

    const dropEnv: DropEnv = {
      item: (id) => store.getState().container.getAtId(id),
      itemView,
      prop,
      isWorn: (id) => store.getState().container.isEquip(id),
      isOpen: (app) => fullUi(ctx)?.wm.isOpen(app) ?? false,
      say,
      text: (tid) => fc.texts.text(tid),
      forbid: () => {
        fullUi(ctx)?.wm.drag.forbid();
      },
      groundPos: (pt) => groundPosition(ctx, pt),
      dropRemoves: () => dropRemoves(),
      send: senders,
      get dialogs() {
        const ui = fullUi(ctx);
        if (!ui) throw new Error('the item dialogs need the UI');
        return ui.dialogs;
      },
    };

    const actions = createInventoryActions({
      use,
      senders,
      drop: dropEnv,
      state: () => store.getState(),
      say,
      setViewMask: (on) => {
        setOptionByPath(options, 'game.viewMask', on);
      },
      dropIn: (sc, fromApp) => dropIns.get(fromApp)?.(sc) ?? false,
    });

    // ---- the taskbar source ----------------------------------------------------------------------------------------------

    const stackable = (itemId: number): boolean => (prop(itemId)?.pack_max ?? 1) > 1;
    const taskbarSource: TaskbarItemSource = {
      ready: () => store.getState().ready,
      has: (objId) => store.getState().container.getAtId(objId) !== null,
      stackItemId: (objId) => {
        const it = store.getState().container.getAtId(objId);
        return it && stackable(it.itemId) ? it.itemId : null;
      },
      find: (itemId) => store.getState().container.find(itemId)?.objId ?? null,
      stackable,
      icon: (objId) => {
        const it = store.getState().container.getAtId(objId);
        const p = it ? prop(it.itemId) : undefined;
        return itemIconPath(p, it ?? undefined);
      },
      count: (objId) => {
        const it = store.getState().container.getAtId(objId);
        return it && stackable(it.itemId) ? store.getState().container.countOf(it.itemId) : null;
      },
      cooldown: (objId) => {
        const it = store.getState().container.getAtId(objId);
        const p = it ? prop(it.itemId) : undefined;
        if (!p) return null;
        const g = cooldownGroup(
          p.item_kind2,
          p.item_kind3,
          p.skill_ready,
          IK2_FOOD,
          IK2_SKILL,
          IK3_PILL,
        );
        return g > 0 ? (store.getState().cooldowns[g - 1] ?? null) : null;
      },
      parts: (objId) => {
        const it = store.getState().container.getAtId(objId);
        return it ? partsOf(prop(it.itemId)) : null;
      },
      view: (objId) => {
        const it = store.getState().container.getAtId(objId);
        return it ? itemView(it) : null;
      },
    };

    const api: InventoryFeature = {
      store,
      view,
      actions,
      use,
      senders,
      itemSenders: glue.senders,
      openSummonAngel: () => {
        fullUi(ctx)?.itemUse.openSummonAngel(summonRequest);
      },
      taskbarSource,
      state: () => store.getState(),
      item: (id) => store.getState().container.getAtId(id),
      equipped: (part) => store.getState().container.getEquip(part),
      countOf: (itemId) => store.getState().container.countOf(itemId),
      find: (itemId) => store.getState().container.find(itemId),
      bulletEquipped: () => {
        const b = store.getState().container.getEquip(PARTS_BULLET);
        return b !== null && prop(b.itemId)?.item_kind3 === IK3_ARROW;
      },
      setExtra: (objId, extra) => {
        store.setState(applyExtra(store.getState(), objId, extra));
      },
      addItem: (item) => {
        const before = store.getState();
        const next = applyAddItem(before, item, prop(item.itemId)?.pack_max ?? 1);
        if (next === before) return false;
        store.setState(next);
        return true;
      },
      setGoldHold: (hold) => {
        const next = Math.max(0, Math.trunc(hold));
        if (next === goldHold) return;
        goldHold = next;
        for (const fn of [...holdListeners]) fn();
      },
      itemView,
      setDropRemoves: (fn) => {
        dropRemoves = fn;
      },
      registerDropIn: (fromApp, fn) => {
        dropIns.set(fromApp, fn);
        return () => {
          if (dropIns.get(fromApp) === fn) dropIns.delete(fromApp);
        };
      },
      registerDispatchWindows: (extra) => {
        // the dispatch reads `env.windows` at the moment of every double click: the members land in the live object
        Object.assign(glue.windows, extra);
      },
      registerDispatchEnv: (extra) => {
        Object.assign(glue.env, extra);
      },
      registerFeed: (app, fn) => {
        feeds.set(app, fn);
        return () => {
          if (feeds.get(app) === fn) feeds.delete(app);
        };
      },
    };
    features.set(ctx, api);
    // `ReturnScrollNtf`: the scroll is used again with the chosen village
    fc.net.on('returnScrollNtf', () => {
      use.router.onReturnScrollNtf();
    });
    // the upgrade cursor sends its request when the wait is over
    ctx.tickSystems.register('inventory.dispatch', () => {
      use.process(ctx.now());
    });
    // the input layer asks before it fires a bow (`IsBullet`)
    inputProvidersOf(ctx).bulletEquipped = () => api.bulletEquipped();
    // the create-monster item waits for a ground click (`g_Neuz.m_pCreateMonItem`): `input/placement.ts` decides, the router sends
    inputProvidersOf(ctx).createMonster = {
      active: () => use.router.createMonsterPending !== null,
      leave: () => {
        use.router.cancelCreateMonster();
      },
      place: (pos) => use.router.placeCreateMonster(pos),
    };

    // ---- handlers ----------------------------------------------------------------------------------------------------------

    const selfRec = () => ctx.entities.self();
    const markStats = (): void => {
      peekRulesRuntime(ctx)?.markSelfStatsDirty();
    };

    fc.net.on('spawnSelf', (m) => {
      store.setState(applyContainer(store.getState(), m.self?.inventory, m.player?.hideCoat));
    });
    fc.net.on('createItem', (m) => {
      if (!m.item) return;
      store.setState(applyCreateItem(store.getState(), itemFromWire(m.item), m.objIds, m.nums));
    });
    fc.net.on('moveItemNtf', (m) => {
      store.setState(applyMoveItem(store.getState(), m.srcSlot, m.dstSlot, rules));
    });
    fc.net.on('updateItem', (m) => {
      let s = store.getState();
      if (m.param === UI.COOLTIME) {
        // the cooldown group of the item starts BEFORE the count is applied (`OnUpdateItem`)
        const it = s.container.getAtId(m.objId);
        const p = it ? prop(it.itemId) : undefined;
        if (p) {
          const g = cooldownGroup(
            p.item_kind2,
            p.item_kind3,
            p.skill_ready,
            IK2_FOOD,
            IK2_SKILL,
            IK3_PILL,
          );
          s = applyCooldown(s, g, ctx.now(), p.skill_ready);
        }
      }
      const worn = s.container.isEquip(m.objId);
      const r = applyUpdateItem(s, m.objId, m.param, m.value, m.time, rules);
      store.setState(r.state);
      if (r.equipFlag) setVisualFlag(selfRec(), r.equipFlag.part, r.equipFlag.flag);
      if (worn) markStats();
    });
    fc.net.on('updateItemEx', (m) => {
      store.setState(applyUpdateItemEx(store.getState(), m.objId, m.param, m.value));
    });
    fc.net.on('doEquipNtf', (m, objId) => {
      const rec = objId === 0 ? selfRec() : ctx.entities.get(objId);
      if (!rec) return;
      const info = m.info;
      if (rec.isSelf) {
        const r = applyEquip(store.getState(), m.objId, m.equip, m.part, rules);
        if (r) {
          store.setState(r.state);
          applyOwnVisual(rec, r.changed);
        } else if (info && rec.visual) {
          // the container did not know the item (an inventory that was not received): the visual still follows the message
          const part = m.part >= 0 ? m.part : partsOf(prop(info.itemId));
          if (part >= 0)
            setVisualPart(
              rec.visual,
              part,
              m.equip ? { itemId: info.itemId, flag: info.flag, option: info.option } : null,
            );
        }
      } else if (rec.kind === EntityKind.Player && rec.visual && info) {
        // `DoFakeEquip`: only the visual equipment table of another player
        const part = m.part >= 0 ? m.part : partsOf(prop(info.itemId));
        if (part >= 0) {
          setVisualPart(
            rec.visual,
            part,
            m.equip ? { itemId: info.itemId, flag: info.flag, option: info.option } : null,
          );
        }
      }
    });
    fc.net.on('hideCoatNtf', (m, objId) => {
      const rec = objId === 0 ? selfRec() : ctx.entities.get(objId);
      if (!rec) return;
      if (m.position >= 0 && m.position <= 4 && rec.visual)
        rec.visual.hideCoat[m.position] = m.hide;
      if (rec.isSelf) store.setState(applyHideCoat(store.getState(), m.position, m.hide));
    });

    // ---- the ports into the UI and the taskbar ---------------------------------------------------------------------------

    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.char.setPorts({ inventory: { view, actions } });
      ui.wm.drag.setWorldDrop((sc, pt) => dropOnWorld(dropEnv, sc, pt));
      // the item tooltip's composer and numbers are the `tooltip` feature's (`features/tooltip`)
      const tb = taskbarOf(ctx);
      tb?.setItems(taskbarSource);
      tb?.setShortcutPorts({
        items: {
          useFromShortcut: (objId) => {
            use.fromShortcut(objId);
          },
        },
      });
    };
    // the free escape command (the motion row `MOT_BASE_ESCAPE`, `SendDoEscape`): sent, except in the guild-war world
    let escapeRegistered = false;
    const ensureEscape = (): void => {
      const motions = motionsOf(ctx);
      if (escapeRegistered || !motions) return;
      escapeRegistered = true;
      motions.register(MOT_BASE_ESCAPE, () => {
        if (selfStore.getState().worldId === WI_WORLD_GUILDWAR) {
          glue.windows.upperBox(TID_GAME_GUILDCOMBAT_NO_OUTWAR);
          return;
        }
        glue.senders.doEscape();
      });
    };
    ctx.tickSystems.register('inventory.escape', ensureEscape);
    ensureEscape();
    ctx.tickSystems.register('inventory.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });
    ensureUi();

    addProbeMember(ctx, 'inventory', () => {
      const s = store.getState();
      const c = s.container;
      const bag: { slot: number; objId: number; itemId: number; num: number; name: string }[] = [];
      for (let slot = 0; slot < c.indexNum; slot += 1) {
        const it = c.getAtSlot(slot);
        if (it)
          bag.push({
            slot,
            objId: it.objId,
            itemId: it.itemId,
            num: it.num,
            name: prop(it.itemId)?.name ?? '',
          });
      }
      const equip: { part: number; objId: number; itemId: number }[] = [];
      for (let part = 0; part < c.max - c.indexNum; part += 1) {
        const it = c.getEquip(part);
        if (it) equip.push({ part, objId: it.objId, itemId: it.itemId });
      }
      return {
        ready: s.ready,
        indexNum: c.indexNum,
        gold: selfStore.getState().gold,
        bag,
        equip,
        cooldowns: s.cooldowns.map((x) => (x ? { endMs: x.endMs } : null)),
        hideCoat: [...s.hideCoat],
        invariants: c.checkInvariants(),
        lastUse: use.last,
      };
    });
  },
};
