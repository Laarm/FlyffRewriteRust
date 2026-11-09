// The `ultimate` feature module (Plan/45 §11.2, §18.6; WP-CFEAT-26): the windows of the ultimate weapons over `ui.ultimate`, the menu actions that open
// them, the double-click feed of the bag items, the `Ultimate` answers and the sends:
//
//   MMI_SMELT_MIXJEWEL (51)      APP_SMELT_MIXJEWEL: five Orichalcum + five moonstones -> `UltimateMakeItem{ids[10]}` (the answer opens the confirmation)
//   MMI_SMELT_JEWEL (52)         APP_SMELT_JEWEL: a jewel into the free socket of an ultimate weapon -> `UltimateSetGem{weapon, gem}`
//   MMI_SMELT_EXTRACTION (53)    APP_SMELT_EXTRACTION: a general or unique weapon -> `UltimateMakeGem{weapon}`
//   MMI_SMELT_CHANGEGWEAPON (54) / MMI_SMELT_CHANGEUWEAPON (55)   APP_SMELT_CHANGEWEAPON: general -> unique / unique -> ultimate -> `UltimateTransWeapon`
//   MMI_SMELT_REMOVE_JEWEL (95)  APP_SMELT_REMOVE_JEWEL (+ APP_SMELT_REMOVE_CONFIRM): the last jewel of an ultimate weapon -> `UltimateRemoveGem`
//
// Every action first answers `TID_GAME_WND_OVERLAPED` (`TID_GAME_CHANGE_NOWUSING` for the weapon change) when the window is open already, then opens the
// bag next to it (the items are dragged out of it). The windows reserve their items (`SetExtra`) and release them when they close; they close when a trade
// or a stall starts. The dispatch learns of the jewel window's weapon (an Orichalcum cannot enchant it meanwhile) and of the unique weapon change (its
// protection scroll may be used meanwhile). `Ultimate{sub, result}` goes to the window of its sub-message; the enchant and the removal answers are read
// and ignored like the original. The piercing sends of the windows of WP-UI-31 are `senders.piercingSize` and `senders.piercingRemove`.

import type { ClientContext } from '../../app/context';
import {
  APP_INVENTORY,
  APP_PIERCING,
  APP_SMELT_CHANGEWEAPON,
  APP_SMELT_EXTRACTION,
  APP_SMELT_JEWEL,
  APP_SMELT_MIXJEWEL,
  APP_SMELT_REMOVE_JEWEL,
  APP_SMELT_REMOVE_PIERCING_EX,
  APP_TRADE,
  APP_VENDOR_REVISION,
} from '../../gen/defs/resdata';
import {
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_ORICHALCUM02,
} from '../../gen/defs/item';
import {
  MMI_SMELT_CHANGEGWEAPON,
  MMI_SMELT_CHANGEUWEAPON,
  MMI_SMELT_EXTRACTION,
  MMI_SMELT_JEWEL,
  MMI_SMELT_MIXJEWEL,
  MMI_SMELT_REMOVE_JEWEL,
} from '../../gen/defs/neuz';
import {
  TID_GAME_CHANGE_NOWUSING,
  TID_GAME_REMOVEJEWEL,
  TID_GAME_WND_OVERLAPED,
} from '../../gen/defs/text';
import type { Ultimate } from '../../gen/proto/flyff/v1/upgrade_pb';
import type { UltimateBase, UltimateItem } from '../../ui/windows/ultimate/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { inventoryOf } from '../inventory';
import type { DispatchWindows } from '../inventory/dispatch/env';
import { ULTIMATE_FAILED, ULTIMATE_SUCCESS } from '../../ui/windows/ultimate/results';
import { createShiftTracker } from '../inventory/dispatch/windows';
import { menuOf } from '../npc';
import { colorsOf } from '../tooltip/colors';
import { ultimateItem } from './facts';
import { gemKind, jewelText } from './gems';
import { registerPiercing, PIERCING_MENU } from './piercing';
import { createUltimateSenders, type UltimateSenders } from './senders';

/** The windows of this feature. */
export type UltimateWindowKind =
  'mix' | 'jewel' | 'extraction' | 'changeGeneral' | 'changeUnique' | 'removeJewel';

/** The `MMI_*` id that opens each window. */
export const ULTIMATE_MENU: Readonly<Record<UltimateWindowKind, number>> = {
  mix: MMI_SMELT_MIXJEWEL,
  jewel: MMI_SMELT_JEWEL,
  extraction: MMI_SMELT_EXTRACTION,
  changeGeneral: MMI_SMELT_CHANGEGWEAPON,
  changeUnique: MMI_SMELT_CHANGEUWEAPON,
  removeJewel: MMI_SMELT_REMOVE_JEWEL,
};

/** The panels of the help bundle (`Client/Smelt*.inc`) with the descriptions of the windows. */
const PANEL = {
  mix: 'smeltmixjewel',
  extraction: 'smeltextraction',
  jewel: 'smeltjewel',
  changeGeneral: 'smeltchangeuniqueweapon',
  changeGeneralInfo: 'changeuniqueweaponinfo',
  changeUnique: 'smeltchangeultimateweapon',
  changeUniqueInfo: 'changeultimateweaponinfo',
  removeJewel: 'smeltremovejewel',
} as const;

/** The windows that end the ultimate windows when they open (a trade partner or the own stall: the legacy `Process`). */
const TRADE_WINDOWS: readonly number[] = [APP_TRADE, APP_VENDOR_REVISION];

/** The windows of this feature and of the piercing shells it wires. */
const GUARDED_WINDOWS: readonly number[] = [
  APP_SMELT_MIXJEWEL,
  APP_SMELT_EXTRACTION,
  APP_SMELT_JEWEL,
  APP_SMELT_CHANGEWEAPON,
  APP_SMELT_REMOVE_JEWEL,
  APP_PIERCING,
  APP_SMELT_REMOVE_PIERCING_EX,
];

export interface UltimateFeature {
  readonly senders: UltimateSenders;
  /** Opens the window of `kind` as its menu action does (the overlap check, the bag, the window); `false` when none opened. */
  open(kind: UltimateWindowKind): boolean;
  /** The bag item `objId` as the windows see it; `null` for a missing or a worn item. */
  item(objId: number): UltimateItem | null;
  /** Counters for the debug probe. */
  readonly stats: {
    opened: Record<string, number>;
    sent: Record<string, number>;
    results: Record<string, number>;
  };
}

const features = new WeakMap<ClientContext, UltimateFeature>();

/** The ultimate feature of a context; `undefined` before the module registered. */
export function ultimateOf(ctx: ClientContext): UltimateFeature | undefined {
  return features.get(ctx);
}

export const ultimateModule: FeatureModule = {
  id: 'ultimate',
  milestone: 4,
  register(fc) {
    registerUltimate(fc);
  },
};

function registerUltimate(fc: FeatureContext): UltimateFeature | undefined {
  const ctx = fc.app;
  const inv = inventoryOf(ctx);
  if (!inv) return undefined;
  const senders = createUltimateSenders(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number, args: readonly (string | number)[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };
  const stats: UltimateFeature['stats'] = { opened: {}, sent: {}, results: {} };
  const count = (rec: Record<string, number>, key: string): void => {
    rec[key] = (rec[key] ?? 0) + 1;
  };
  const sent = (name: string): void => {
    count(stats.sent, name);
  };
  const panel = (key: string): string => ctx.data?.bundle('help')?.panels[key] ?? '';
  /**
   * The descriptions (`Client/Smelt*.inc`) are panels of the lazy `help` bundle, which nothing else loads: ask for it at every open
   * (de-duplicated; a failure is not an error: the windows then show no description). NOT when the module registers: a lazy bundle is
   * never fetched at boot (`boot.test`), and a gated server refuses it before the login.
   */
  const prefetchHelp = (): void => {
    if (ctx.bundles.has('help')) return;
    ctx.bundles.ensure('help').catch(() => undefined);
  };

  // ---- the bag ------------------------------------------------------------------------------------------------------------------

  /** A worn item is not a candidate (the server would answer `TID_GAME_ULTIMATE_ISEQUIP`): the window shows the forbid cursor. */
  const readItem = (objId: number): UltimateItem | null => {
    const it = inv.item(objId);
    if (!it || inv.state().container.isEquip(objId)) return null;
    const view = inv.itemView(it);
    if (!view) return null;
    return ultimateItem(it, ctx.data?.item(it.itemId), view);
  };
  const iconOfItem = (id: number): string | null => {
    const p = ctx.data?.item(id);
    return p && p.icon !== '' ? `item/${p.icon}` : null;
  };
  const base = (): UltimateBase => ({
    item: readItem,
    setExtra: (objId, extra) => {
      inv.setExtra(objId, extra);
    },
    messageBox: (tid) => {
      fullUi(ctx)?.msgbox.open(text(tid));
    },
    say,
  });
  const improved = (): boolean => ctx.features.features.improved_ulti_gems;

  // ---- the windows ---------------------------------------------------------------------------------------------------------------

  const shiftDown = createShiftTracker();

  const openMix = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    return (
      ui.ultimate.openMixJewel({
        ...base(),
        description: panel(PANEL.mix),
        icons: {
          orichalcum: iconOfItem(II_GEN_MAT_ORICHALCUM01),
          moonstone: iconOfItem(II_GEN_MAT_MOONSTONE),
          shining: iconOfItem(II_GEN_MAT_ORICHALCUM02),
        },
        shiftDown,
        make: (ids) => {
          senders.makeItem(ids);
          sent('makeItem');
        },
      }) !== null
    );
  };

  const openExtraction = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    return (
      ui.ultimate.openExtraction({
        ...base(),
        description: panel(PANEL.extraction),
        gemName: (level) => {
          const id = gemKind(level);
          return id === null ? '' : (ctx.data?.item(id)?.name ?? '');
        },
        make: (weapon) => {
          senders.makeGem(weapon);
          sent('makeGem');
        },
      }) !== null
    );
  };

  const openJewel = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    return (
      ui.ultimate.openSmeltJewel({
        ...base(),
        description: panel(PANEL.jewel),
        iconOf: iconOfItem,
        set: (weapon, gem) => {
          senders.setGem(weapon, gem);
          sent('setGem');
        },
        started: () => {
          ctx.audio.cues?.upgradeResult('smeltStart');
        },
      }) !== null
    );
  };

  const openChange = (type: 'general' | 'unique'): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    return (
      ui.ultimate.openChangeWeapon({
        ...base(),
        type,
        description: panel(type === 'general' ? PANEL.changeGeneral : PANEL.changeUnique),
        info: panel(type === 'general' ? PANEL.changeGeneralInfo : PANEL.changeUniqueInfo),
        gemKind,
        transform: (weapon, gem, shining) => {
          senders.transWeapon(weapon, gem, shining);
          sent('transWeapon');
        },
      }) !== null
    );
  };

  const openRemove = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    const colors = () => colorsOf(fc.options.getState().ui.tooltipColorSet);
    return (
      ui.ultimate.openRemoveJewel({
        ...base(),
        description: panel(PANEL.removeJewel),
        confirmText: text(TID_GAME_REMOVEJEWEL),
        moonstoneIcon: iconOfItem(II_GEN_MAT_MOONSTONE),
        jewelIcon: iconOfItem,
        jewelLabel: (id) => jewelText(id, improved(), colors(), text),
        remove: (weapon, moonstone) => {
          senders.removeGem(weapon, moonstone);
          sent('removeGem');
        },
      }) !== null
    );
  };

  const APP_OF: Readonly<Record<UltimateWindowKind, number>> = {
    mix: APP_SMELT_MIXJEWEL,
    jewel: APP_SMELT_JEWEL,
    extraction: APP_SMELT_EXTRACTION,
    changeGeneral: APP_SMELT_CHANGEWEAPON,
    changeUnique: APP_SMELT_CHANGEWEAPON,
    removeJewel: APP_SMELT_REMOVE_JEWEL,
  };

  const open = (kind: UltimateWindowKind): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    prefetchHelp();
    // `GetWndBase( APP_X )`: one window of a kind at a time
    if (ui.wm.isOpen(APP_OF[kind])) {
      say(
        kind === 'changeGeneral' || kind === 'changeUnique'
          ? TID_GAME_CHANGE_NOWUSING
          : TID_GAME_WND_OVERLAPED,
      );
      return false;
    }
    // the items are dragged out of the bag
    if (!ui.wm.isOpen(APP_INVENTORY)) ui.wm.openApplet(APP_INVENTORY);
    let opened: boolean;
    switch (kind) {
      case 'mix':
        opened = openMix();
        break;
      case 'jewel':
        opened = openJewel();
        break;
      case 'extraction':
        opened = openExtraction();
        break;
      case 'changeGeneral':
        opened = openChange('general');
        break;
      case 'changeUnique':
        opened = openChange('unique');
        break;
      case 'removeJewel':
        opened = openRemove();
        break;
    }
    if (opened) count(stats.opened, kind);
    return opened;
  };

  // ---- the menu actions ---------------------------------------------------------------------------------------------------------

  const piercing = registerPiercing({
    fc,
    ctx,
    inv,
    senders,
    text,
    say: (tid) => {
      say(tid);
    },
    count: sent,
  });
  const menu = menuOf(ctx);
  if (menu) {
    menu.registerAction(PIERCING_MENU.piercing, () => {
      if (piercing.openPiercing()) count(stats.opened, 'piercing');
    });
    menu.registerAction(PIERCING_MENU.removePiercing, () => {
      if (piercing.openRemove()) count(stats.opened, 'removePiercing');
    });
    for (const kind of Object.keys(ULTIMATE_MENU) as UltimateWindowKind[]) {
      menu.registerAction(ULTIMATE_MENU[kind], () => {
        open(kind);
      });
    }
  }

  // ---- the dispatch -------------------------------------------------------------------------------------------------------------

  const feed = (app: number, fn: (objId: number) => boolean): void => {
    inv.registerFeed(app, (item) => fn(item.objId));
  };
  feed(APP_SMELT_MIXJEWEL, (id) => fullUi(ctx)?.ultimate.mixJewel()?.feed(id) ?? false);
  feed(APP_SMELT_EXTRACTION, (id) => fullUi(ctx)?.ultimate.extraction()?.feed(id) ?? false);
  feed(APP_SMELT_JEWEL, (id) => fullUi(ctx)?.ultimate.smeltJewel()?.feed(id) ?? false);
  feed(APP_SMELT_CHANGEWEAPON, (id) => fullUi(ctx)?.ultimate.changeWeapon()?.feed(id) ?? false);
  feed(APP_SMELT_REMOVE_JEWEL, (id) => fullUi(ctx)?.ultimate.removeJewel()?.feed(id) ?? false);
  // `APP_SMELT_REMOVE_PIERCING(_EX)` <- any item (rule D3; the window checks the item and prints the refusal)
  feed(
    APP_SMELT_REMOVE_PIERCING_EX,
    (id) => fullUi(ctx)?.upgrade.removePiercing()?.setItem(id) ?? false,
  );
  const windows: Pick<DispatchWindows, 'smeltJewelItem' | 'changeWeaponUnique'> = {
    smeltJewelItem: () => fullUi(ctx)?.ultimate.smeltJewel()?.weapon?.objId ?? null,
    changeWeaponUnique: () => fullUi(ctx)?.ultimate.changeWeapon()?.type === 'unique',
  };
  inv.registerDispatchWindows(windows);

  // ---- the answers --------------------------------------------------------------------------------------------------------------

  /** T30: `InfUpgradeSuccess.wav` / `InfUpgradeFail.wav` for the answer of the jewel setting and the weapon change window (the other answers are silent). */
  const resultSound = (result: number): void => {
    if (result === ULTIMATE_SUCCESS) ctx.audio.cues?.upgradeResult('success');
    else if (result === ULTIMATE_FAILED) ctx.audio.cues?.upgradeResult('fail');
  };

  fc.net.on('ultimate', (m: Ultimate) => {
    const ui = fullUi(ctx);
    const sub = m.sub;
    if (sub.case !== undefined) count(stats.results, `${sub.case}:${String(sub.value.result)}`);
    switch (sub.case) {
      case 'makeItem':
        ui?.ultimate.mixJewel()?.receiveResult(sub.value.result);
        break;
      case 'transWeapon': {
        const w = ui?.ultimate.changeWeapon();
        if (w) resultSound(sub.value.result);
        w?.receiveResult(sub.value.result);
        break;
      }
      case 'makeGem':
        ui?.ultimate.extraction()?.receiveResult(sub.value.result, sub.value.num);
        break;
      case 'setGem': {
        const w = ui?.ultimate.smeltJewel();
        if (w) resultSound(sub.value.result);
        w?.receiveResult(sub.value.result);
        break;
      }
      case 'removeGem':
      case 'enchantWeapon':
      case undefined:
        // `REMOVEGEM` and `ENCHANTWEAPON`: the original reads the result and ignores it
        break;
    }
  });

  // the help panels are fetched once the player is in the world (the bag has arrived: the login is done, a gated server serves the bundle)
  let helpAsked = false;
  ctx.tickSystems.register('ultimate.help', () => {
    if (helpAsked || !inv.state().ready) return;
    helpAsked = true;
    prefetchHelp();
  });

  // a trade or a stall ends the ultimate windows (the server refuses them meanwhile; the legacy `Process` of the upgrade windows closes them)
  ctx.tickSystems.register('ultimate.guard', () => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (!TRADE_WINDOWS.some((app) => ui.wm.isOpen(app)) && !ui.wm.isVendorWindowOpen()) return;
    for (const app of GUARDED_WINDOWS) ui.wm.get(app)?.destroy();
  });

  const feature: UltimateFeature = { senders, open, item: readItem, stats };
  features.set(ctx, feature);
  addProbeMember(ctx, 'ultimate', () => ({
    opened: { ...stats.opened },
    sent: { ...stats.sent },
    results: { ...stats.results },
    mix: windowRow(ctx, APP_SMELT_MIXJEWEL),
    jewel: windowRow(ctx, APP_SMELT_JEWEL),
    extraction: windowRow(ctx, APP_SMELT_EXTRACTION),
    change: windowRow(ctx, APP_SMELT_CHANGEWEAPON),
    removeJewel: windowRow(ctx, APP_SMELT_REMOVE_JEWEL),
  }));
  // the debug entry of the menu actions: the NPC menus that offer the mixing, extraction and weapon change windows are commented out in the shipped
  // `character.inc`, so the browser tests open them through the same function the menu action calls
  addProbeMember(ctx, 'openUltimate', (kind?: UltimateWindowKind) =>
    kind !== undefined && kind in ULTIMATE_MENU ? open(kind) : false,
  );
  return feature;
}

/** What the probe says of an open window: whether it is open. */
function windowRow(ctx: ClientContext, app: number): { open: boolean } {
  return { open: fullUi(ctx)?.wm.isOpen(app) ?? false };
}
