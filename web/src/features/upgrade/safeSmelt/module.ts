// The safe-upgrade windows on the client (Plan/45 §18.2, §11.2, §9.2 D3; WP-CFEAT-24): the NPC menus `MMI_SMELT_SAFETY_GENERAL / _ACCESSORY / _PIERCING / _ELEMENT`
// open `APP_SMELT_SAFETY`, the auto window (`features.better_safe_upgrades`) or the one-shot window of a mode (PL-169); the double click of an inventory item feeds the
// open window (rule D3); `SmeltSafetyNtf` goes to the machine; the window closes itself when a trade opens or the player starts vending.

import { APP_SMELT_SAFETY } from '../../../gen/defs/resdata';
import {
  MMI_SMELT_SAFETY_ACCESSORY,
  MMI_SMELT_SAFETY_ELEMENT,
  MMI_SMELT_SAFETY_GENERAL,
  MMI_SMELT_SAFETY_PIERCING,
} from '../../../gen/defs/neuz';
import { TID_GAME_SMELT_SAFETY_ERROR16 } from '../../../gen/defs/text';
import type { SmeltSafetyActions, SmeltSafetyRequest } from '../../../ui/windows/upgrade/ports';
import type { InventoryFeature } from '../../inventory';
import { isPierceAble } from '../../inventory/dispatch/upgradeRules';

const NULL_ID = 0xffff_ffff;
import type { InvItem } from '../../inventory/items';
import type { MenuFeature } from '../../npc/menuModule';
import type { UpgradeCommon } from '../common';
import type { UpgradeSenders } from '../senders';
import { AutoSmelt } from './autoSmelt';
import { OneShotSmelt } from './oneShotSmelt';
import type { OneShotMode } from './rules';
import type { SmeltEnv, SmeltMachine } from './types';

export interface SafeSmeltFeature {
  /** `MMI_SMELT_SAFETY_GENERAL`: the auto window, or the one-shot window of the normal mode without `better_safe_upgrades`. */
  openGeneral(): void;
  /** The one-shot window of a mode (the accessory, piercing and element NPC menus). */
  openOneShot(mode: OneShotMode): void;
  /** The machine behind the open window, `null` when none is open. */
  machine(): SmeltMachine | null;
  /** Closes the open window. */
  close(): void;
  /** What was opened, sent and answered (the probe). */
  stats(): { opened: number; sent: number; results: number };
}

/** The environment of a machine over the client. */
export function createSmeltEnv(
  common: UpgradeCommon,
  send: SmeltEnv['send'],
  gold: () => number,
): SmeltEnv {
  const { ctx, inv } = common;
  return {
    now: () => ctx.now(),
    enchantMs: () => ctx.features.gameplay.enchant_time_s * 1000,
    prop: (id) => ctx.data?.item(id),
    item: (objId) => inv.item(objId),
    stacks: (itemId) => bagStacks(inv, itemId),
    setExtra: (objId, extra) => {
      inv.setExtra(objId, extra);
    },
    isWorn: (objId) => inv.state().container.isEquip(objId),
    icon: (itemId) => {
      const p = ctx.data?.item(itemId);
      return p && p.icon !== '' ? `item/${p.icon}` : null;
    },
    dialogItem: (item) => inv.itemView(item),
    pierceAble: (item) => {
      const p = ctx.data?.item(item.itemId);
      return p !== undefined && isPierceAble(item, p, NULL_ID, ctx.features.features.no_piercing);
    },
    gold,
    send,
    say: (line) => {
      common.sayRaw(line, 0xffffffff);
    },
    sayTid: (tid) => {
      common.say(tid);
    },
    text: (tid) => common.text(tid),
  };
}

/** Every bag stack of an item id in object-id order (`GetAtItemId` loops). */
function bagStacks(inv: InventoryFeature, itemId: number): readonly InvItem[] {
  const c = inv.state().container;
  const out: InvItem[] = [];
  for (const it of c.items) {
    if (it?.itemId === itemId && !c.isEquip(it.objId)) out.push(it);
  }
  return out;
}

export function registerSafeSmelt(
  common: UpgradeCommon,
  menu: MenuFeature,
  senders: UpgradeSenders,
): SafeSmeltFeature {
  const { ctx, inv, fc } = common;
  let current: SmeltMachine | null = null;
  const stats = { opened: 0, sent: 0, results: 0 };
  const env = createSmeltEnv(
    common,
    (m) => {
      stats.sent += 1;
      senders.smeltSafety(m);
    },
    () => Math.max(0, ctx.stores.selfStore.getState().gold),
  );

  const close = (): void => {
    const ui = common.ui();
    if (current) {
      const m = current;
      current = null;
      m.dispose();
    }
    ui?.upgrade.smeltSafety()?.destroy();
  };

  const open = (machine: SmeltMachine): void => {
    const ui = common.ui();
    if (!ui) return;
    if (common.tradingOrVending()) {
      common.say(TID_GAME_SMELT_SAFETY_ERROR16);
      return;
    }
    if (current) {
      const old = current;
      current = null;
      old.dispose();
    }
    current = machine;
    const actions: SmeltSafetyActions = {
      drop: (objId) => {
        machine.setItem(objId);
      },
      clear: (slot) => {
        machine.clear(slot);
      },
      start: () => {
        machine.start();
      },
      stop: () => {
        machine.stop();
      },
      reset: () => {
        machine.reset();
      },
      adjust: (d) => {
        machine.adjust(d);
      },
      closed: () => {
        // a replacement already disposed the old machine
        if (current === machine) current = null;
        machine.dispose();
      },
    };
    const request: SmeltSafetyRequest = {
      view: { get: () => machine.view(), subscribe: () => () => undefined },
      actions,
    };
    // `CreateApplet( APP_INVENTORY )`: the bag is there to drag from
    ui.char.openInventory();
    ui.upgrade.openSmeltSafety(request);
    stats.opened += 1;
  };

  const general = (): void => {
    if (ctx.features.features.better_safe_upgrades) open(new AutoSmelt(env));
    else open(new OneShotSmelt(env, 'normal'));
  };

  menu.registerAction(MMI_SMELT_SAFETY_GENERAL, general);
  menu.registerAction(MMI_SMELT_SAFETY_ACCESSORY, () => {
    open(new OneShotSmelt(env, 'jewel'));
  });
  menu.registerAction(MMI_SMELT_SAFETY_PIERCING, () => {
    open(new OneShotSmelt(env, 'piercing'));
  });
  menu.registerAction(MMI_SMELT_SAFETY_ELEMENT, () => {
    open(new OneShotSmelt(env, 'element'));
  });

  // rule D3: the open window takes the item of a double click (the original returns "handled" whatever `SetItem` answered)
  inv.registerFeed(APP_SMELT_SAFETY, (item) => {
    current?.setItem(item.objId);
    return true;
  });

  // `SmeltSafetyNtf`
  fc.net.on('smeltSafetyNtf', (m) => {
    stats.results += 1;
    current?.onResult(m.result);
  });

  // the pacing and `Process`'s "close when trading or vending"
  ctx.tickSystems.register('upgrade.safeSmelt', () => {
    if (!current) return;
    if (common.tradingOrVending()) {
      close();
      return;
    }
    current.tick();
  });

  return {
    openGeneral: general,
    openOneShot: (mode) => {
      open(new OneShotSmelt(env, mode));
    },
    machine: () => current,
    close,
    stats: () => ({ ...stats }),
  };
}
