// The awakening windows on the client (Plan/45 §18.4; WP-CFEAT-25): `APP_AWAKENING` (`MMI_ITEM_AWAKENING`, `Awakening{obj_id}`), `APP_CANCEL_BLESSING`
// (`MMI_BLESSING_CANCEL`, `BlessednessCancel{obj_id}`), `APP_LVREQDOWN` (`MMI_LVREQDOWN_CANCEL`, `RemoveItemLevelDown{obj_id}`), `APP_PET_AWAK_CANCEL`
// (`MMI_PET_AWAK_CANCEL`, `PickupPetAwakeningCancel{obj_id}`) and the choice of a protected awakening `APP_AWAKE_SELECTCASE`
// (`ItemSelectAwakeningValueNtf{obj_id, serial, new_word}` opens it - also the copy the server re-sends after a login while the choice is pending,
// PL-079 - and the buttons answer `ItemSelectAwakeningValueReq{obj_id, serial, 1 | 2}`). The windows reserve their item and release it when they close.

import { APP_AWAKENING, APP_CANCEL_BLESSING, APP_LVREQDOWN } from '../../../gen/defs/resdata';
import {
  MMI_BLESSING_CANCEL,
  MMI_ITEM_AWAKENING,
  MMI_LVREQDOWN_CANCEL,
  MMI_PET_AWAK_CANCEL,
} from '../../../gen/defs/neuz';
import { IK3_PET } from '../../../gen/defs/itemkind';
import {
  TID_GAME_AWAKE_OR_BLESSEDNESS01,
  TID_GAME_BLESSEDNESS_CANCEL,
  TID_GAME_BLESSEDNESS_CANCEL_INFO,
  TID_GAME_EQUIPPUT,
  TID_GAME_INVALID_TARGET_ITEM,
  TID_GAME_REGARDLESS_USE02,
  TID_GAME_REGARDLESS_USE03,
} from '../../../gen/defs/text';
import type { SelectAwakeRequest, StartItemRequest } from '../../../ui/windows/upgrade/ports';
import type { MenuFeature } from '../../npc/menuModule';
import type { UpgradeCommon } from '../common';
import { freeOrSay } from '../common';
import type { UpgradeSenders } from '../senders';

/** `AWAKE_KEEP_TIME` (WndAwakening.cpp): 60 s to choose. */
export const AWAKE_KEEP_MS = 60_000;
/** The colours of `PutString` in the originals: the red refusal and the blue confirmation of the blessing cancel. */
const RED = 0xffff0000;
const BLUE = 0xff0000ff;

export interface AwakenStats {
  awakenings: number;
  cancels: number;
  levelDowns: number;
  petCancels: number;
  choices: number;
}

export interface AwakenFeature {
  /** The pending protected choice that is being shown, `null` when none. */
  pendingChoice(): { objId: number; serial: bigint } | null;
  /** What was sent (the probe). */
  stats(): AwakenStats;
}

export function registerAwaken(
  common: UpgradeCommon,
  menu: MenuFeature,
  senders: UpgradeSenders,
): AwakenFeature {
  const { ctx, inv, fc } = common;
  let choice: { objId: number; serial: bigint } | null = null;
  const stats: AwakenStats = {
    awakenings: 0,
    cancels: 0,
    levelDowns: 0,
    petCancels: 0,
    choices: 0,
  };

  /** The part of every request that is the same: the item view, the reservation, the worn-item refusal. */
  const base = (
    description: string,
    fits: (objId: number) => boolean,
  ): Omit<StartItemRequest, 'send'> => ({
    description,
    item: (objId) => common.dialogItem(objId),
    reserve: (objId, on) => {
      common.reserve(objId, on);
    },
    accept: (objId) => {
      if (inv.state().container.isEquip(objId)) {
        common.say(TID_GAME_EQUIPPUT);
        return false;
      }
      return fits(objId);
    },
  });

  // ---- the awakening --------------------------------------------------------------------------------------------------------------

  const awakeningRequest = (): StartItemRequest => ({
    ...base(common.panel('itemawakening'), (objId) => {
      const it = inv.item(objId);
      if (!it) return false;
      // `GetRandomOptionKind != eAwakening` and `GetRandomOptionSize > 0`: two message boxes
      if (common.optionKind(it) !== 'awakening') {
        common.msgbox(common.text(TID_GAME_INVALID_TARGET_ITEM));
        return false;
      }
      if (common.optionLines(it).length > 0) {
        common.msgbox(common.text(TID_GAME_AWAKE_OR_BLESSEDNESS01));
        return false;
      }
      return true;
    }),
    send: (objId) => {
      stats.awakenings += 1;
      senders.awakening(objId);
    },
  });

  menu.registerAction(MMI_ITEM_AWAKENING, () => {
    if (!freeOrSay(common, APP_AWAKENING)) return;
    common.ui()?.upgrade.openAwakening(awakeningRequest());
  });

  // ---- the blessing cancel --------------------------------------------------------------------------------------------------------

  const blessingRequest = (): StartItemRequest => ({
    ...base(common.panel('itemblessingcancel'), (objId) => {
      const it = inv.item(objId);
      // only an item in the lowered state (a blessing kind with lines) goes in; anything else gets the red line
      if (it && common.optionKind(it) === 'blessing' && common.optionLines(it).length > 0) {
        return true;
      }
      common.sayRaw(common.text(TID_GAME_BLESSEDNESS_CANCEL), RED);
      return false;
    }),
    send: (objId) => {
      stats.cancels += 1;
      senders.blessednessCancel(objId);
      common.sayRaw(common.text(TID_GAME_BLESSEDNESS_CANCEL_INFO), BLUE);
    },
  });

  menu.registerAction(MMI_BLESSING_CANCEL, () => {
    if (!freeOrSay(common, APP_CANCEL_BLESSING)) return;
    common.ui()?.upgrade.openCancelBlessing(blessingRequest());
  });

  // ---- the required-level reduction -----------------------------------------------------------------------------------------------

  const lvReqRequest = (): StartItemRequest => ({
    ...base(common.panel('reqlvdown'), (objId) => {
      const it = inv.item(objId);
      // `GetLevelDown() < 0`: nothing is said for another item
      return it !== null && common.levelDown(it) < 0;
    }),
    send: (objId) => {
      stats.levelDowns += 1;
      senders.removeItemLevelDown(objId);
    },
  });

  menu.registerAction(MMI_LVREQDOWN_CANCEL, () => {
    if (!freeOrSay(common, APP_LVREQDOWN)) return;
    common.ui()?.upgrade.openLvReqDown(lvReqRequest());
  });

  // ---- the pick-up pet awakening ---------------------------------------------------------------------------------------------------

  const petRequest = (): StartItemRequest => ({
    ...base(common.panel('petawakcancel'), (objId) => {
      const it = inv.item(objId);
      // `IsEatPet()`; whether it is awakened is the server's to check
      return it !== null && ctx.data?.item(it.itemId)?.item_kind3 === IK3_PET;
    }),
    send: (objId) => {
      stats.petCancels += 1;
      senders.pickupPetAwakeningCancel(objId);
    },
  });

  // the original always replaces an open window of this kind
  menu.registerAction(MMI_PET_AWAK_CANCEL, () => {
    common.ui()?.upgrade.openPetAwakCancel(petRequest());
  });

  // ---- the protected awakening ---------------------------------------------------------------------------------------------------

  fc.net.on('itemSelectAwakeningValueNtf', (m) => {
    const ui = common.ui();
    const it = inv.item(m.itemObjId);
    if (!ui || !it) return;
    const dialog = common.dialogItem(m.itemObjId);
    if (!dialog) return;
    const serial = m.serial;
    const objId = m.itemObjId;
    const request: SelectAwakeRequest = {
      item: dialog,
      oldLines: common.optionLines(it).map((l) => common.lineText(l)),
      newLines: common.optionLines(it, m.newRandomOption).map((l) => common.lineText(l)),
      durationMs: AWAKE_KEEP_MS,
      now: () => ctx.now(),
      choose: (flag) => {
        stats.choices += 1;
        senders.selectAwakening(objId, serial, flag);
        common.say(TID_GAME_REGARDLESS_USE03);
      },
      timeout: () => {
        stats.choices += 1;
        senders.selectAwakening(objId, serial, 1);
        common.say(TID_GAME_REGARDLESS_USE02);
      },
      closed: () => {
        if (choice?.objId === objId) choice = null;
      },
    };
    choice = { objId, serial };
    ui.upgrade.openSelectAwakeCase(request);
  });

  return { pendingChoice: () => choice, stats: () => ({ ...stats }) };
}
