// The `itemuse` feature module (Plan/45 §9.6, §18.3, §18.5; WP-CFEAT-15): the item-specific use windows the item dispatch (rules D6, R8, R19,
// R24, R30, D8) asks for through the optional members of `DispatchWindows`, over the windows of `ui.itemUse` (WP-UI-28 and the new shells of
// WP-CFEAT-15):
//
//   removeElem         `APP_REMOVE_ELEM`: replacing a worn weapon / suit with an element stone asks first; OK runs the use / equip again
//   restateConfirm     `APP_RESTATE_CONFIRM`: the re-stat scroll with the text of the scroll; Yes uses it (`SendDoUseItem`, target 0 as the original's
//                      self-assignment leaves it, `bResult = TRUE`); the server answers with `SetState`
//   changeAttribute    `APP_CHANGE_ATTRIBUTE`: the element-change scroll: target slot, five icons, OK sends `ChangeAttribute{item, scroll, 1..5}`
//   randomOptionRetry  `APP_RANDOMOPTIONRETRY`: the awakening augmentation scroll: a slot and the lines of the item; Start sends `RandomOptionRetry`
//   funnyCoinConfirm   `APP_FUNNYCOIN_CONFIRM`: "use <item>?"; Yes sends `DoUseItem` without a target and with `bResult = FALSE`
//   confirmEnter       `APP_CONFIRM_ENTER`: vis / vis key / pick-up pet to buff pet / awakening protection; OK sends `DoUseItem` with the own object
//   selectChannel      `APP_SELECT_CHANNEL`: the ticket layer list; a choice sends `DoUseItemInput` with `"0"`, `"-1"` ...
//   changeName         `APP_CHANGENAME`: the pet naming scroll (`DoUseItemInput` with the name; the name rules of the lobby run first) and the character
//                      rename scroll (`QuerySetPlayerNameReq`; the answer `QuerySetPlayerNameNtf` is the messenger's, WP-CFEAT-39)
//   pictureMap         `II_SYS_SYS_MAP_*`: `CWndMgr::OpenMap` toggles the world map in this build (`__INSERT_MAP`), as the key M does
//
// The skill-reset, blessing and class-change confirms of rule D6 are generic `CWndCommItemDlg` boxes of the dispatch itself. The ticket tables
// (`ticketOf`) have no client data yet (the ticket system is server side M7): the selector is unit-tested only.

import type { ClientContext } from '../../app/context';
import { APP_MAP, APP_RANDOMOPTIONRETRY } from '../../gen/defs/resdata';
import { TID_DIAG_0020, TID_DIAG_RULE_0 } from '../../gen/defs/text';
import {
  TID_GAME_BUFFPET_EQUIP,
  TID_GAME_BUFFPET_KET01,
  TID_GAME_CHAR_SERVERNAME,
  TID_GAME_EQUIPPUT,
  TID_GAME_FUNNYCOIN_ASKUSE,
  TID_GAME_NOTELEMENT,
  TID_GAME_PET_TRAN,
  TID_GAME_REGARDLESS_USE01,
  TID_GAME_REMOVEELEM,
  TID_MMI_RANDOMOPTIONRETRY_ERRORTEXT03,
  TID_TOOLTIP_CA_EARTH,
  TID_TOOLTIP_CA_ELEC,
  TID_TOOLTIP_CA_FIRE,
  TID_TOOLTIP_CA_INSLOT,
  TID_TOOLTIP_CA_WATER,
  TID_TOOLTIP_CA_WIND,
} from '../../gen/defs/text';
import type { ItemTooltipNumbers } from '../../rules/types';
import type {
  ChangeAttributeRequest,
  ChangeNameRequest,
  RandomOptionRetryRequest,
  SelectChannelRequest,
  YesNoTextRequest,
} from '../../ui/windows/itemuse/ports';
import { checkPartyName } from '../party/names';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { nameRulesBytes } from '../common/lobbyRules';
import { addProbeMember } from '../common/probe';
import { formatTid, sysChat } from '../common/text';
import { inventoryOf } from '../inventory';
import { RESTATE_TID } from '../inventory/dispatch/doubleClick';
import type { DispatchWindows } from '../inventory/dispatch/env';
import { isEleRefineryAble } from '../inventory/dispatch/upgradeRules';
import type { InvItem } from '../inventory/items';
import { dstAwakeLine } from '../tooltip/dst';
import { randomOptionKind } from '../tooltip/sections/awakening';
import { tooltipOf } from '../tooltip/module';
import { createItemUseWindowSenders, type ItemUseWindowSenders } from './senders';

const NULL_ID = 0xffff_ffff;
/** The element sprites of the attribute window: `Icon_MonElemantkind.inc`, 16 x 16 each, fire first. */
const ELEMENT_ATLAS = 'icon/icon_monelemantkind.inc';
const ELEMENT_TEXTURE = 'icon/icon_monelemantkind.dds';
/** The panel of the help bundle (`Client/ChangeAttribute.inc`) with the description of the element-change scroll. */
const CHANGE_ATTRIBUTE_PANEL = 'changeattribute';
/** `GetRandomOptionSize` is at most three for an awakening. */
const MAX_AWAKE_LINES = 3;

/** The members of `DispatchWindows` this module provides. */
type ItemUseWindowKey =
  | 'removeElem'
  | 'restateConfirm'
  | 'changeAttribute'
  | 'randomOptionRetry'
  | 'funnyCoinConfirm'
  | 'confirmEnter'
  | 'selectChannel'
  | 'changeName'
  | 'pictureMap';

export type ConfirmEnterKind = 'vis' | 'viskey' | 'pickupToBuff' | 'protectAwake';

export interface ItemUseFeature {
  readonly senders: ItemUseWindowSenders;
  /** The windows this module registered with the dispatch (the tests drive them directly). */
  readonly windows: {
    readonly [K in ItemUseWindowKey]-?: NonNullable<DispatchWindows[K]>;
  };
  /** The eat pet's name for the vis-key question (`GetEatPetId` -> `pProp->szName`); installed by the pet feature, `''` without one. */
  setEatPetName(fn: () => string): void;
}

const features = new WeakMap<ClientContext, ItemUseFeature>();

/** The item-use feature of a context; `undefined` before the module registered. */
export function itemUseOf(ctx: ClientContext): ItemUseFeature | undefined {
  return features.get(ctx);
}

/** The text of the confirm of an `APP_CONFIRM_ENTER` kind (`CWndConfirmVis::OnInitialUpdate`). */
export function confirmEnterText(
  kind: ConfirmEnterKind,
  fc: Pick<FeatureContext, 'texts'>,
  itemName: string,
  petName: string,
): string {
  switch (kind) {
    case 'vis':
      return formatTid(fc.texts, TID_GAME_BUFFPET_EQUIP, [itemName]);
    case 'viskey':
      // the original leaves the text empty when no eat pet is out
      return petName === '' ? '' : formatTid(fc.texts, TID_GAME_BUFFPET_KET01, [petName]);
    case 'pickupToBuff':
      return fc.texts.text(TID_GAME_PET_TRAN);
    case 'protectAwake':
      return fc.texts.text(TID_GAME_REGARDLESS_USE01);
  }
}

export const itemuseModule: FeatureModule = {
  id: 'itemuse',
  milestone: 4,
  register(fc) {
    registerItemUse(fc);
  },
};

function registerItemUse(fc: FeatureContext): ItemUseFeature | undefined {
  const ctx = fc.app;
  const inv = inventoryOf(ctx);
  if (!inv) return undefined;
  const senders = createItemUseWindowSenders(ctx);
  const router = inv.use.router;
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
  };
  let eatPetName: () => string = () => '';
  const stats = { opened: {} as Record<string, number>, sent: 0 };
  const opened = (name: string): void => {
    stats.opened[name] = (stats.opened[name] ?? 0) + 1;
  };

  const fresh = (item: InvItem): InvItem | null => inv.item(item.objId);

  /** `SetExtra(GetExtra() + 1)` / `SetExtra(0)`. */
  const reserve = (objId: number, on: boolean): void => {
    const it = inv.item(objId);
    if (!it) return;
    inv.setExtra(objId, on ? it.extra + 1 : 0);
  };
  const dialogItem = (it: InvItem | null) => {
    if (!it) return null;
    const view = inv.itemView(it);
    return view;
  };

  // ---- the Yes / No windows ------------------------------------------------------------------------------------------------------

  const removeElem: ItemUseFeature['windows']['removeElem'] = (_item, _target, _part, proceed) => {
    const request: YesNoTextRequest = { text: text(TID_GAME_REMOVEELEM), yes: proceed };
    fullUi(ctx)?.itemUse.openRemoveElem(request);
    opened('removeElem');
  };

  const restateConfirm: ItemUseFeature['windows']['restateConfirm'] = (item, _target, part) => {
    const tid = RESTATE_TID.get(item.itemId);
    if (tid === undefined) return;
    fullUi(ctx)?.itemUse.openRestateConfirm({
      text: text(tid),
      yes: () => {
        const cur = fresh(item);
        // `SendDoUseItem(item, m_ObjID, nPart)`: `m_ObjID` is the constructor's 0 (`SetInformation` assigns the parameter to itself)
        if (cur) router.useItem(cur, 0, part, true);
      },
    });
    opened('restateConfirm');
  };

  const funnyCoinConfirm: ItemUseFeature['windows']['funnyCoinConfirm'] = (item) => {
    const name = ctx.data?.item(item.itemId)?.name ?? '';
    fullUi(ctx)?.itemUse.openFunnyCoinConfirm({
      text: formatTid(fc.texts, TID_GAME_FUNNYCOIN_ASKUSE, [name]),
      yes: () => {
        const cur = fresh(item);
        // `SendDoUseItem(item, NULL_ID, -1, FALSE)`
        if (cur) router.useItem(cur, NULL_ID, -1, false);
      },
    });
    opened('funnyCoinConfirm');
  };

  const confirmEnter: ItemUseFeature['windows']['confirmEnter'] = (kind, item) => {
    const name = ctx.data?.item(item.itemId)?.name ?? '';
    fullUi(ctx)?.itemUse.openConfirmEnter({
      text: confirmEnterText(kind, fc, name, eatPetName()),
      yes: () => {
        const cur = fresh(item);
        const own = ctx.entities.self()?.objId ?? NULL_ID;
        // `SendDoUseItem(item, g_pPlayer->GetId())` through the router (the default `bResult`)
        if (cur) router.useItem(cur, own, -1, true);
      },
    });
    opened('confirmEnter');
  };

  // ---- the ticket layer list ------------------------------------------------------------------------------------------------------

  const selectChannel: ItemUseFeature['windows']['selectChannel'] = (item, _world, expanded) => {
    const request: SelectChannelRequest = {
      count: expanded + 1,
      label: text(TID_GAME_CHAR_SERVERNAME),
      choose: (key) => {
        const cur = fresh(item);
        if (cur) inv.itemSenders.doUseItemInput(cur.objId, key);
      },
    };
    fullUi(ctx)?.itemUse.openSelectChannel(request);
    opened('selectChannel');
  };

  // ---- the name edit --------------------------------------------------------------------------------------------------------------

  /** `IsValidPlayerName`, then `IsInvalidName` / `IsAllowedLetter`: the first refusing text id, `0` for a good name. */
  const nameProblem = (name: string): number => {
    if (name === '') return TID_DIAG_RULE_0;
    const names = ctx.data?.bundle('names');
    const tid = checkPartyName(name, fc.lobbyRules, names ? nameRulesBytes(names) : null);
    return tid > 0 ? tid : 0;
  };

  const changeName: ItemUseFeature['windows']['changeName'] = (item, mode) => {
    const request: ChangeNameRequest = {
      validate: nameProblem,
      error: (tid) => {
        fullUi(ctx)?.msgbox.open(text(tid === 0 ? TID_DIAG_0020 : tid));
      },
      confirm: (name) => {
        const cur = fresh(item);
        if (!cur) return;
        // the rename scroll of the character: `SendQuerySetPlayerName( MAKELONG( m_dwId, 0 ), name )` (the answer `QuerySetPlayerNameNtf` is the messenger's, WP-CFEAT-39);
        // the pet naming scroll: `SendDoUseItemInput(m_dwId, name)`
        if (mode === 'character') senders.querySetPlayerName(cur.objId, name);
        else inv.itemSenders.doUseItemInput(cur.objId, name);
      },
    };
    fullUi(ctx)?.itemUse.openChangeName(request);
    opened('changeName');
  };

  // ---- the element-change scroll ----------------------------------------------------------------------------------------------------

  const elementCut = (i: number): { x: number; y: number } => {
    const sprite = ctx.bundles.get('ui-atlases')?.atlases[ELEMENT_ATLAS]?.sprites[i];
    return sprite ? { x: sprite.x, y: sprite.y } : { x: i * 16, y: 0 };
  };

  const changeAttribute: ItemUseFeature['windows']['changeAttribute'] = (item) => {
    const scroll = dialogItem(fresh(item));
    const ui = fullUi(ctx);
    if (!scroll || !ui) return;
    const panel = ctx.bundles.get('help')?.panels[CHANGE_ATTRIBUTE_PANEL] ?? '';
    const request: ChangeAttributeRequest = {
      scroll,
      description: panel,
      tips: [
        TID_TOOLTIP_CA_INSLOT,
        TID_TOOLTIP_CA_FIRE,
        TID_TOOLTIP_CA_WATER,
        TID_TOOLTIP_CA_ELEC,
        TID_TOOLTIP_CA_WIND,
        TID_TOOLTIP_CA_EARTH,
      ].map(text),
      elementIcons: { path: ELEMENT_TEXTURE, cut: elementCut },
      item: (objId) => dialogItem(inv.item(objId)),
      eligible: (objId) => {
        const it = inv.item(objId);
        const prop = it ? ctx.data?.item(it.itemId) : undefined;
        // `IsEleRefineryAble(prop) && m_nResistAbilityOption > 0`
        return (
          it !== null && prop !== undefined && isEleRefineryAble(prop) && it.resistAbilityOption > 0
        );
      },
      reserve,
      send: (itemObjId, scrollObjId, attribute) => {
        senders.changeAttribute(itemObjId, scrollObjId, attribute);
        stats.sent += 1;
      },
      refused: () => {
        say(TID_GAME_NOTELEMENT);
      },
    };
    ui.itemUse.openChangeAttribute(request);
    opened('changeAttribute');
  };

  // ---- the awakening augmentation scroll ----------------------------------------------------------------------------------------------

  /** The awakening lines of an item as the radio buttons print them (`"<name> +<n>"`), `null` for an item that is no awakening target. */
  const awakeLines = (objId: number): readonly string[] | null => {
    const it = inv.item(objId);
    const prop = it ? ctx.data?.item(it.itemId) : undefined;
    if (!it || !prop) return null;
    const kind = randomOptionKind(
      prop.parts,
      prop.item_kind3,
      (it.pet?.level ?? 0) >= 2,
      ctx.features.features.only_armor_awakes,
    );
    if (kind !== 'awakening') return null;
    const view = inv.itemView(it);
    const answer = view
      ? ((tooltipOf(ctx)?.numbers(view, { flag: APP_RANDOMOPTIONRETRY, now: ctx.now() })?.detail ??
          null) as ItemTooltipNumbers | null)
      : null;
    const lines = answer?.randomOptions ?? [];
    return lines
      .slice(0, MAX_AWAKE_LINES)
      .map((l) => dstAwakeLine(l.dst, l.adj, (tid) => text(tid)).trim());
  };

  const randomOptionRetry: ItemUseFeature['windows']['randomOptionRetry'] = (item) => {
    const scroll = dialogItem(fresh(item));
    const ui = fullUi(ctx);
    if (!scroll || !ui) return;
    const listeners = new Set<() => void>();
    const off = inv.store.subscribe(() => {
      for (const fn of [...listeners]) fn();
    });
    const request: RandomOptionRetryRequest = {
      scroll,
      item: (objId) => dialogItem(inv.item(objId)),
      lines: awakeLines,
      reserve,
      subscribe: (fn) => {
        listeners.add(fn);
        return () => {
          listeners.delete(fn);
          if (listeners.size === 0) off();
        };
      },
      send: (itemObjId, scrollObjId, line) => {
        senders.randomOptionRetry(itemObjId, scrollObjId, line);
        stats.sent += 1;
      },
      refusedItem: () => {
        ui.msgbox.open(text(TID_MMI_RANDOMOPTIONRETRY_ERRORTEXT03));
      },
      worn: (objId) => inv.state().container.isEquip(objId),
      sayWorn: () => {
        say(TID_GAME_EQUIPPUT);
      },
      defaults: ['Awake Option 1', 'Awake Option 2', 'Awake Option 3'],
    };
    ui.itemUse.openRandomOptionRetry(request);
    opened('randomOptionRetry');
  };

  // ---- the picture maps ---------------------------------------------------------------------------------------------------------------

  /** `CWndMgr::OpenMap(file)`: with `__INSERT_MAP` the window's `Initialize` toggles the world map instead of opening a window. */
  const pictureMap: ItemUseFeature['windows']['pictureMap'] = () => {
    fullUi(ctx)?.wm.objectExecutorApplet(APP_MAP);
  };

  const windows: ItemUseFeature['windows'] = {
    removeElem,
    restateConfirm,
    changeAttribute,
    randomOptionRetry,
    funnyCoinConfirm,
    confirmEnter,
    selectChannel,
    changeName,
    pictureMap,
  };
  inv.registerDispatchWindows(windows);

  const feature: ItemUseFeature = {
    senders,
    windows,
    setEatPetName(fn) {
      eatPetName = fn;
    },
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'itemuse', () => ({ stats: { ...stats, opened: { ...stats.opened } } }));
  return feature;
}
