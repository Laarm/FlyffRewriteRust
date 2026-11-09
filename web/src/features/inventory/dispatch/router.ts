// The item-use router (Plan/45 §9.5, WP-CFEAT-14; `CDPClient::SendDoUseItem`, `src:Neuz/DPClient.cpp:10329-10945`, and
// `SendDoEquip` :10952-11045). `useItem(item, target, part, userAction)` runs the steps R0-R31 in the original's order: a step
// either aborts (nothing is sent), opens a window that sends later, or lets the next step run. `userAction` is the original's
// `bResult` (`TRUE` for a double click or an equip, `FALSE` for the re-send after a confirmation).
//
// Windows that are not built yet (WP-CFEAT-15 and later) are ports of `DispatchWindows`: a step whose window is absent refuses with
// `TID_SBEVE_NOTUSEITEM` instead of sending an unconfirmed request. `last` is the debug probe's `router.lastDecision`.

import { EXT_PET } from '../../../gen/defs/define';
import {
  IK1_RIDE,
  IK2_BLINKWING,
  IK2_TOCASH,
  IK3_ARROW,
  IK3_BOW,
  IK3_CREATE_MONSTER,
  IK3_TEXT_DISGUISE,
  IK3_TICKET,
  IK2_FOOD,
  IK2_SKILL,
  IK3_PILL,
} from '../../../gen/defs/itemkind';
import {
  II_CHR_MAG_TRI_ANGELWING,
  II_CHR_SYS_SCR_ITEMTRANSY_A,
  II_CHR_SYS_SCR_ITEMTRANSY_B,
  II_CHR_SYS_SCR_TRANSY,
  II_GEN_TOO_COL_BATTERY001,
  II_GEN_TOO_COL_GOLDBATTERY,
  II_GEN_TOO_COL_NORMALBATTERY,
  II_GEN_TOO_COL_SILVERBATTERY,
  II_SYS_SYS_EVE_WINGS,
  II_SYS_SYS_FEED_01,
  II_SYS_SYS_FEED_MAKER,
  II_SYS_SYS_SCR_CHANAM,
  II_SYS_SYS_SCR_CUSTODY,
  II_SYS_SYS_SCR_FACEOFFFREE,
  II_SYS_SYS_SCR_GCHANAM,
  II_SYS_SYS_SCR_HAIRCHANGE,
  II_SYS_SYS_SCR_PARTYSUMMON,
  II_SYS_SYS_SCR_PET_HATCH,
  II_SYS_SYS_SCR_PET_LIFE,
  II_SYS_SYS_SCR_PET_NAMING,
  II_SYS_SYS_SCR_PET_TAMER_MIRACLE,
  II_SYS_SYS_SCR_PET_TAMER_MISTAKE,
  II_SYS_SYS_SCR_PIEPROT,
  II_SYS_SYS_SCR_RETURN,
  II_SYS_SYS_SCR_SMELPROT,
  II_SYS_SYS_SCR_SMELPROT3,
  II_SYS_SYS_SCR_SMELPROT4,
  II_SYS_SYS_SCR_SMELTING,
  II_SYS_SYS_SCR_SMELTING2,
  II_SYS_SYS_SCR_SUPERSMELTING,
} from '../../../gen/defs/item';
import {
  PARTS_BULLET,
  PARTS_CAP,
  PARTS_HAIR,
  PARTS_HAT,
  PARTS_HEAD,
  PARTS_LWEAPON,
  PARTS_NECKLACE1,
  PARTS_RIDE,
  PARTS_RWEAPON,
  PARTS_SHIELD,
  PARTS_UPPER_BODY,
} from '../../../gen/defs/neuz';
import {
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_COMMON_BANK,
  APP_PET_FOODMILL,
  APP_SHOPEX,
  APP_SMELT_CHANGEWEAPON,
  APP_TRADE,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_ATTENTIONCOOLTIME,
  TID_GAME_BETTERY_DESC,
  TID_GAME_CACHE_BETTERY_DESC,
  TID_GAME_CHECK_EQUIP,
  TID_GAME_DISQUISE_DNT_WING,
  TID_GAME_DONOTUSE_SHOP,
  TID_GAME_GUILDCOMBAT_NO_OUTWAR,
  TID_GAME_LIMITED_USE,
  TID_GAME_LV10OVER_NOTUSE,
  TID_GAME_LV7OVER_NOTUSE,
  TID_GAME_NOTUSESKILL_ITEMUSE,
  TID_GAME_ONLYPIER_USE,
  TID_GAME_PET_HATCH_DESC,
  TID_GAME_PET_MISTAKE_DESC,
  TID_GAME_PET_NOT_FOUND,
  TID_GAME_REGARDLESS_USE,
  TID_GAME_REMOVEELEM,
  TID_GAME_TICKET_DESC,
  TID_GAME_TRADELIMITUSING,
  TID_GAME_TROUPECREATE,
  TID_GAME_WND_OVERLAPED,
  TID_PK_BLINK_NO,
  TID_QUEST_DISQUISE_NOTFLY,
  TID_SBEVE_NOTUSEITEM,
} from '../../../gen/defs/text';
import { WI_WORLD_GUILDWAR, WI_WORLD_KEBARAS, WI_WORLD_MINIROOM } from '../../../gen/defs/world';
import type { ClientItem } from '../../../data/clientData';
import { BUFF_ITEM } from '../../buffs/types';
import { ITEM_FLAG, type InvItem } from '../items';
import { cooldownGroup, cooldownRunning } from '../state';
import type { DispatchEnv } from './env';

/** One decision of the router (`router.lastDecision`): the step and what it did. */
export interface Decision {
  readonly step: string;
  readonly outcome: string;
}

const NULL_ID = 0xffff_ffff;

/** The trade windows that forbid using an item from a confirm (`CWndCommItemDlg::OnChildNotify`). */
const TRADE_WINDOWS: readonly number[] = [APP_SHOPEX, APP_COMMON_BANK, APP_TRADE];

export class ItemRouter {
  readonly #env: DispatchEnv;
  /** `g_Neuz.m_pCreateMonItem`: the create-monster item waiting for a ground click. */
  #createMonster: InvItem | null = null;
  /** `m_dwReturnScroll`: the object id of the return scroll waiting for `ReturnScrollNtf`. */
  #returnScroll: number | null = null;
  last: Decision | null = null;
  /** Tells the dispatcher about every decision (it keeps the one `last` of both). */
  onDecision: ((d: Decision) => void) | null = null;

  constructor(env: DispatchEnv) {
    this.#env = env;
  }

  /** The create-monster item that waits for a ground click (R26). */
  get createMonsterPending(): InvItem | null {
    return this.#createMonster;
  }

  /** The cursor was cancelled (Esc, a click on a window): `g_Neuz.m_pCreateMonItem = NULL`. */
  cancelCreateMonster(): void {
    this.#createMonster = null;
  }

  #decide(step: string, outcome: string): void {
    this.last = { step, outcome };
    this.onDecision?.(this.last);
  }

  #say(tid: number, args?: readonly (string | number)[]): void {
    this.#env.say(tid, args);
  }

  #refuseNotBuilt(step: string): void {
    this.#say(TID_SBEVE_NOTUSEITEM);
    this.#decide(step, 'not built');
  }

  /**
   * The re-send of a confirmed use: `CWndCommItemDlg` Yes. The trade windows still forbid the use; a ticket sends its channel
   * input; the rest goes through the router again without the confirms (`userAction = false`, part -1).
   */
  #confirmedUse(objId: number, target: number): void {
    const e = this.#env;
    if (TRADE_WINDOWS.some((a) => e.isOpen(a))) {
      this.#say(TID_GAME_TRADELIMITUSING);
      return;
    }
    const item = e.state().container.getAtId(objId);
    if (!item) return;
    // a ticket sends its layer key ("0", the base layer) as the input of the use
    if (e.prop(item.itemId)?.item_kind3 === IK3_TICKET) {
      e.send.doUseItemInput(item.objId, '0');
      return;
    }
    this.useItem(item, target, -1, false);
  }

  /** `CDPClient::OnReturnScrollACK`: the answer of `ReturnScrollReq` uses the scroll without a window. */
  onReturnScrollNtf(): void {
    const id = this.#returnScroll;
    if (id === null) return;
    const item = this.#env.state().container.getAtId(id);
    if (item) this.useItem(item, NULL_ID, -1, false);
  }

  /** The ground click of a pending create-monster item (`WndWorldControlPlayer.cpp:245-285`): `true` when it was sent. */
  placeCreateMonster(pos: readonly [number, number, number]): boolean {
    const item = this.#createMonster;
    if (!item) return false;
    this.#createMonster = null;
    this.#env.send.createMonster(item.objId, pos);
    return true;
  }

  /** `SendDoUseItem(data, target, part, bResult)`: the steps R0-R31. */
  useItem(item: InvItem, target: number, part: number, userAction: boolean): void {
    const e = this.#env;
    const self = e.self();
    // R0
    if (!self.present || self.dead) {
      this.#decide('R0', 'abort');
      return;
    }
    const cur = e.state().container.getAtId(item.objId);
    if (!cur) {
      this.#decide('R0', 'abort');
      return;
    }
    const prop = e.prop(cur.itemId);
    if (!prop) {
      this.#decide('R0', 'abort');
      return;
    }
    const id = prop.id;
    const dataItem = cur;
    const send = (): void => {
      e.send.doUseItem(dataItem.objId, target, part);
    };
    // R1: no hat or cap while a beauty shop is open
    if (
      (prop.parts === PARTS_HAT || prop.parts === PARTS_CAP) &&
      (e.isOpen(APP_BEAUTY_SHOP_EX) || e.isOpen(APP_BEAUTY_SHOP_SKIN))
    ) {
      this.#say(TID_GAME_DONOTUSE_SHOP);
      this.#decide('R1', 'refused');
      return;
    }
    // R2: the weapon-change window takes the unique-weapon protection scroll only
    if (e.isOpen(APP_SMELT_CHANGEWEAPON)) {
      const unique = e.windows.changeWeaponUnique?.() === true;
      if (!unique || id !== II_SYS_SYS_SCR_SMELPROT3) {
        this.#say(TID_SBEVE_NOTUSEITEM);
        this.#decide('R2', 'refused');
        return;
      }
    }
    // R3: another item than the create-monster item cancels the pending cursor
    let justCancelled = false;
    if (this.#createMonster && prop.item_kind3 !== IK3_CREATE_MONSTER) {
      this.#createMonster = null;
      justCancelled = true;
    }
    // R4: the cooldown group runs
    const group = cooldownGroup(
      prop.item_kind2,
      prop.item_kind3,
      prop.skill_ready,
      IK2_FOOD,
      IK2_SKILL,
      IK3_PILL,
    );
    if (group > 0 && cooldownRunning(e.state(), group, e.now())) {
      this.#say(TID_GAME_ATTENTIONCOOLTIME);
      this.#decide('R4', 'cooldown');
      return;
    }
    // R5: the cast time of a timed item has not been reached
    if (!e.ready(cur, prop)) {
      this.#decide('R5', 'cast');
      return;
    }
    // R6: a disguise forbids rides and wings
    if (e.hasBuffByIk3(IK3_TEXT_DISGUISE)) {
      if (prop.parts === PARTS_RIDE) {
        this.#say(TID_QUEST_DISQUISE_NOTFLY);
        this.#decide('R6', 'refused');
        return;
      }
      if (id === II_CHR_MAG_TRI_ANGELWING || id === II_SYS_SYS_EVE_WINGS) {
        this.#say(TID_GAME_DISQUISE_DNT_WING);
        this.#decide('R6', 'refused');
        return;
      }
    }
    // R7: a wing buff forbids a disguise item and the other wing item
    const angel = e.hasBuff(BUFF_ITEM, II_CHR_MAG_TRI_ANGELWING);
    const evWings = e.hasBuff(BUFF_ITEM, II_SYS_SYS_EVE_WINGS);
    if (angel || evWings) {
      if (prop.item_kind3 === IK3_TEXT_DISGUISE) {
        this.#say(TID_GAME_DISQUISE_DNT_WING);
        this.#decide('R7', 'refused');
        return;
      }
      const clash = angel ? id === II_SYS_SYS_EVE_WINGS : id === II_CHR_MAG_TRI_ANGELWING;
      if (clash) {
        this.#say(TID_GAME_NOTUSESKILL_ITEMUSE);
        this.#decide('R7', 'refused');
        return;
      }
    }
    // R8: replacing a weapon or suit that carries an element stone asks first
    if (
      userAction &&
      (part === PARTS_LWEAPON || part === PARTS_RWEAPON || part === PARTS_UPPER_BODY)
    ) {
      const old = e.state().container.getEquip(part);
      if (old && old.resistSmItemId !== 0) {
        const again = (): void => {
          const fresh = e.state().container.getAtId(dataItem.objId);
          if (fresh) this.useItem(fresh, target, part, false);
        };
        if (e.windows.removeElem) e.windows.removeElem(cur, target, part, again);
        else e.windows.commItemConfirm(TID_GAME_REMOVEELEM, again);
        this.#decide('R8', 'confirm');
        return;
      }
    }
    // R9: a blink wing does not work in some worlds
    if (prop.item_kind2 === IK2_BLINKWING) {
      if (self.worldId === WI_WORLD_KEBARAS || self.worldId === WI_WORLD_MINIROOM) {
        this.#say(TID_PK_BLINK_NO, [self.worldName]);
        this.#decide('R9', 'refused');
        return;
      }
      if (self.worldId === WI_WORLD_GUILDWAR) {
        e.windows.upperBox(TID_GAME_GUILDCOMBAT_NO_OUTWAR);
        this.#decide('R9', 'box');
        return;
      }
    }
    // R10: the return scroll asks for the village first
    if (userAction && id === II_SYS_SYS_SCR_RETURN) {
      if (e.hasBuff(BUFF_ITEM, II_SYS_SYS_SCR_RETURN)) {
        this.#say(TID_GAME_LIMITED_USE);
        this.#decide('R10', 'refused');
        return;
      }
      this.#returnScroll = cur.objId;
      e.windows.returnScroll((select) => {
        e.send.returnScrollReq(select);
      });
      this.#decide('R10', 'select village');
      return;
    }
    // R11: the protection scrolls ask whether to use them at all
    if (
      userAction &&
      (id === II_SYS_SYS_SCR_SMELPROT ||
        id === II_SYS_SYS_SCR_SMELPROT3 ||
        id === II_SYS_SYS_SCR_SMELPROT4)
    ) {
      e.windows.commItemConfirm(TID_GAME_REGARDLESS_USE, () => {
        this.#confirmedUse(cur.objId, target);
      });
      this.#decide('R11', 'confirm');
      return;
    }
    // R12: the smelting scrolls
    if (userAction && (id === II_SYS_SYS_SCR_SMELTING || id === II_SYS_SYS_SCR_SUPERSMELTING)) {
      e.windows.commItemConfirm(TID_GAME_LV7OVER_NOTUSE, () => {
        this.#confirmedUse(cur.objId, target);
      });
      this.#decide('R12', 'confirm');
      return;
    }
    if (userAction && id === II_SYS_SYS_SCR_SMELTING2) {
      e.windows.commItemConfirm(TID_GAME_LV10OVER_NOTUSE, () => {
        this.#confirmedUse(cur.objId, target);
      });
      this.#decide('R12', 'confirm');
      return;
    }
    // R13: the upper protection scroll is used on the piercing window only
    if (id === II_SYS_SYS_SCR_PIEPROT) {
      this.#say(TID_GAME_ONLYPIER_USE);
      this.#decide('R13', 'refused');
      return;
    }
    // R14: the hair and make-up coupons open the beauty shop in coupon mode
    if (id === II_SYS_SYS_SCR_HAIRCHANGE || id === II_SYS_SYS_SCR_FACEOFFFREE) {
      if (!e.windows.beautyCoupon) {
        this.#refuseNotBuilt('R14');
        return;
      }
      e.windows.beautyCoupon(
        id === II_SYS_SYS_SCR_HAIRCHANGE ? APP_BEAUTY_SHOP_EX : APP_BEAUTY_SHOP_SKIN,
      );
      this.#decide('R14', 'window');
      return;
    }
    // R15, R16, R17
    if (id === II_SYS_SYS_SCR_CHANAM) {
      if (!e.windows.changeName) {
        this.#refuseNotBuilt('R15');
        return;
      }
      e.windows.changeName(cur, 'character');
      this.#decide('R15', 'window');
      return;
    }
    if (id === II_SYS_SYS_SCR_CUSTODY) {
      e.send.openBankWnd(cur.objId, cur.itemId);
      this.#decide('R16', 'sent');
      return;
    }
    if (id === II_SYS_SYS_SCR_GCHANAM) {
      if (!e.windows.guildName) {
        this.#refuseNotBuilt('R17');
        return;
      }
      e.windows.guildName(cur);
      this.#decide('R17', 'window');
      return;
    }
    // R18: a ride closes the mover's context menu and continues
    if (prop.item_kind1 === IK1_RIDE) e.windows.hideMoverMenu?.();
    // R19: the funny coin
    if (userAction && prop.item_kind2 === IK2_TOCASH) {
      if (!e.windows.funnyCoinConfirm) {
        this.#refuseNotBuilt('R19');
        return;
      }
      e.windows.funnyCoinConfirm(cur);
      this.#decide('R19', 'window');
      return;
    }
    // R20: the sex-change scroll needs the armour off
    if (id === II_CHR_SYS_SCR_TRANSY && part === -1) {
      const c = e.state().container;
      for (let p = 0; p < c.max - c.indexNum; p += 1) {
        if (
          p === PARTS_HEAD ||
          p === PARTS_HAIR ||
          p === PARTS_LWEAPON ||
          p === PARTS_RWEAPON ||
          p === PARTS_SHIELD ||
          p === PARTS_RIDE ||
          (p >= PARTS_NECKLACE1 && p <= PARTS_BULLET)
        ) {
          continue;
        }
        if (c.getEquip(p)) {
          this.#say(TID_GAME_CHECK_EQUIP);
          this.#decide('R20', 'refused');
          return;
        }
      }
      if (!e.windows.changeSex) {
        this.#refuseNotBuilt('R20');
        return;
      }
      e.windows.changeSex(cur, target);
      this.#decide('R20', 'window');
      return;
    }
    // R22: a pet item without a pet
    if (prop.exe_target === EXT_PET && !(e.hasPet?.() ?? false)) {
      this.#say(TID_GAME_PET_NOT_FOUND);
      this.#decide('R22', 'refused');
      return;
    }
    // R23: the pet and battery items ask or open their windows
    if (userAction) {
      const pet = this.#petItem(cur, prop, target);
      if (pet) {
        this.#decide('R23', pet);
        return;
      }
    }
    // R24: a ticket: in its own world a confirm, elsewhere the layer selector (FIX PL-124: nothing else is sent)
    if (prop.item_kind3 === IK3_TICKET && (cur.flag & ITEM_FLAG.EXPIRED) === 0) {
      const ticket = e.ticketOf?.(id) ?? null;
      if (ticket) {
        if (ticket.world === self.worldId) {
          e.windows.commItemConfirm(TID_GAME_TICKET_DESC, () => {
            this.#confirmedUse(cur.objId, target);
          });
          this.#decide('R24', 'confirm');
          return;
        }
        if (!e.windows.selectChannel) {
          this.#refuseNotBuilt('R24');
          return;
        }
        e.windows.selectChannel(cur, ticket.world, ticket.expandedLayers);
        this.#decide('R24', 'select channel');
        return;
      }
    }
    // R25
    if (id === II_CHR_SYS_SCR_ITEMTRANSY_A || id === II_CHR_SYS_SCR_ITEMTRANSY_B) {
      if (!e.windows.itemTransy) {
        this.#refuseNotBuilt('R25');
        return;
      }
      e.windows.itemTransy(cur);
      this.#decide('R25', 'window');
      return;
    }
    // R26: the create-monster item waits for the ground click
    if (prop.item_kind3 === IK3_CREATE_MONSTER) {
      if (!justCancelled && this.#createMonster === null) {
        this.#createMonster = cur;
        e.windows.cursorSfx?.();
      }
      this.#decide('R26', 'pending');
      return;
    }
    // R27
    if (id === II_SYS_SYS_SCR_PARTYSUMMON) {
      if (!self.partyLeader) {
        this.#say(TID_GAME_TROUPECREATE);
        this.#decide('R27', 'refused');
        return;
      }
      if (!e.windows.summonParty) {
        this.#refuseNotBuilt('R27');
        return;
      }
      e.windows.summonParty(cur);
      this.#decide('R27', 'window');
      return;
    }
    // R28: a new right-hand weapon that is no bow takes the arrows off first
    this.#arrowsOff(prop, (bullet) => {
      e.send.doUseItem(bullet.objId, bullet.objId, PARTS_BULLET);
    });
    // R29: the seal scrolls have no windows (PL-144): the server answers `TID_SBEVE_NOTUSEITEM`
    // R30: the pet naming scroll
    if (id === II_SYS_SYS_SCR_PET_NAMING) {
      if (!e.windows.changeName) {
        this.#refuseNotBuilt('R30');
        return;
      }
      e.windows.changeName(cur, 'pet');
      this.#decide('R30', 'window');
      return;
    }
    // R31
    send();
    this.#decide('R31', 'sent');
  }

  /** R23: returns the outcome when the item was taken over. */
  #petItem(cur: InvItem, prop: ClientItem, target: number): string | null {
    const e = this.#env;
    const yes = (): void => {
      this.#confirmedUse(cur.objId, target);
    };
    switch (prop.id) {
      case II_SYS_SYS_SCR_PET_TAMER_MIRACLE:
        if (!e.windows.petMiracle) {
          this.#say(TID_SBEVE_NOTUSEITEM);
          return 'not built';
        }
        e.windows.petMiracle(cur);
        return 'window';
      case II_SYS_SYS_SCR_PET_TAMER_MISTAKE:
        e.windows.commItemConfirm(TID_GAME_PET_MISTAKE_DESC, yes);
        return 'confirm';
      case II_SYS_SYS_SCR_PET_HATCH:
        e.windows.commItemConfirm(TID_GAME_PET_HATCH_DESC, yes);
        return 'confirm';
      case II_SYS_SYS_FEED_01:
        e.send.usePetFeed(cur.objId);
        return 'sent';
      case II_SYS_SYS_FEED_MAKER:
        if (e.isOpen(APP_PET_FOODMILL)) {
          this.#say(TID_GAME_WND_OVERLAPED);
          return 'refused';
        }
        if (!e.windows.petFoodMill?.(cur)) {
          this.#say(TID_SBEVE_NOTUSEITEM);
          return 'not built';
        }
        return 'window';
      case II_SYS_SYS_SCR_PET_LIFE:
        if (!e.windows.petLifeConfirm) {
          this.#say(TID_SBEVE_NOTUSEITEM);
          return 'not built';
        }
        e.windows.petLifeConfirm(cur);
        return 'window';
      case II_GEN_TOO_COL_NORMALBATTERY:
        e.windows.commItemConfirm(TID_GAME_BETTERY_DESC, yes);
        return 'confirm';
      case II_GEN_TOO_COL_BATTERY001:
      case II_GEN_TOO_COL_SILVERBATTERY:
      case II_GEN_TOO_COL_GOLDBATTERY:
        e.windows.commItemConfirm(TID_GAME_CACHE_BETTERY_DESC, yes);
        return 'confirm';
      default:
        return null;
    }
  }

  /** R28: with a bow in the right hand and a new right-hand weapon that is not a bow, the arrows come off first. */
  #arrowsOff(prop: ClientItem, send: (bullet: InvItem) => void): void {
    if (prop.parts !== PARTS_RWEAPON) return;
    const c = this.#env.state().container;
    const worn = c.getEquip(PARTS_RWEAPON);
    const wornProp = worn ? this.#env.prop(worn.itemId) : undefined;
    if (wornProp?.item_kind3 !== IK3_BOW || prop.item_kind3 === IK3_BOW) return;
    const bullet = c.getEquip(PARTS_BULLET);
    if (bullet && this.#env.prop(bullet.itemId)?.item_kind3 === IK3_ARROW) send(bullet);
  }

  /**
   * `SendDoEquip(item, part, bResult)`: wearing (`part = -1`) or taking off (`part` = the worn part). A weapon or suit that carries an
   * element stone asks before it is replaced (`userAction`); a ride under a disguise, a hat in the beauty shop and a cast time stop
   * it; arrows come off before a non-bow right-hand weapon.
   */
  equip(item: InvItem, part: number, userAction = true): void {
    const e = this.#env;
    const self = e.self();
    if (!self.present) return;
    const prop = e.prop(item.itemId);
    if (!prop) return;
    if (
      userAction &&
      (part === PARTS_LWEAPON || part === PARTS_RWEAPON || part === PARTS_UPPER_BODY)
    ) {
      const old = e.state().container.getEquip(part);
      if (old && old.resistSmItemId !== 0) {
        const again = (): void => {
          this.equip(item, part, false);
        };
        if (e.windows.removeElem) e.windows.removeElem(item, NULL_ID, part, again);
        else e.windows.commItemConfirm(TID_GAME_REMOVEELEM, again);
        return;
      }
    }
    if (!e.ready(item, prop)) return;
    if (prop.parts === PARTS_RIDE && e.hasBuffByIk3(IK3_TEXT_DISGUISE)) {
      this.#say(TID_QUEST_DISQUISE_NOTFLY);
      return;
    }
    if (
      (prop.parts === PARTS_HAT || prop.parts === PARTS_CAP) &&
      (e.isOpen(APP_BEAUTY_SHOP_EX) || e.isOpen(APP_BEAUTY_SHOP_SKIN))
    ) {
      this.#say(TID_GAME_DONOTUSE_SHOP);
      return;
    }
    this.#arrowsOff(prop, (bullet) => {
      e.send.doEquip(bullet.objId, PARTS_BULLET);
    });
    e.send.doEquip(item.objId, part);
  }
}
