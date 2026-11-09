// The windows of the item dispatch over the UI (`DispatchWindows` of `env.ts`; Plan/45 §9.4; WP-CFEAT-13, -14, WP-UI-28): the generic
// "use the item?" confirm on `APP_COMMITEM_DIALOG`, the equip-bind confirm, the return-scroll message box and the village choice, the
// upper message boxes, the random-scroll and smelt-safety confirms. Windows of later work packages (WP-CFEAT-15 and later) plug in
// through the optional members of `DispatchWindows`; until then the dispatch refuses their items with `TID_SBEVE_NOTUSEITEM`.

import {
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_MOONSTONE_1,
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_ORICHALCUM01_1,
} from '../../../gen/defs/item';
import {
  TID_GAME_RANDOMSCROLL_CONFIRM,
  TID_GAME_RANDOMSCROLL_CONFIRM1,
  TID_GAME_RETURN_USEITEM,
  TID_GAME_SMELT_SAFETY_CONFIRM_ACCESSARY,
  TID_GAME_SMELT_SAFETY_CONFIRM_HIGHEST,
  TID_GAME_SMELT_SAFETY_CONFIRM_NORMAL,
} from '../../../gen/defs/text';
import type { AngelStack, SummonAngelRequest } from '../../../ui/windows/itemuse/ports';
import type { ClientItem, TextApi } from '../../../data/clientData';
import type { UiApi } from '../../../ui';
import { MB, MBID } from '../../../ui/msgbox/types';
import { formatTid } from '../../common/text';
import type { InvItem } from '../items';
import type { DispatchWindows } from './env';

export interface WindowDeps {
  /** The full UI, `null` on a title screen (every window call then does nothing). */
  ui(): UiApi | null;
  readonly texts: TextApi;
  prop(itemId: number): ClientItem | undefined;
}

const SMELT_SAFETY_TEXT = {
  1: TID_GAME_SMELT_SAFETY_CONFIRM_NORMAL,
  2: TID_GAME_SMELT_SAFETY_CONFIRM_HIGHEST,
  3: TID_GAME_SMELT_SAFETY_CONFIRM_ACCESSARY,
} as const;

/** The dispatch windows over `ui.msgbox` and `ui.itemUse`; `extra` brings the optional windows of other work packages. */
export function createDispatchWindows(
  deps: WindowDeps,
  extra: Partial<DispatchWindows> = {},
): DispatchWindows {
  const text = (tid: number): string => deps.texts.text(tid);
  return {
    commItemConfirm(tid, yes) {
      deps.ui()?.msgbox.confirm(text(tid), yes);
    },
    equipBindConfirm(yes) {
      deps.ui()?.itemUse.openEquipBindConfirm({ yes });
    },
    returnScroll(onSelect) {
      const ui = deps.ui();
      if (!ui) return;
      // `CReturnScrollMsgBox` (an OK / Cancel box), then `CWndSelectVillage`
      ui.msgbox.open(text(TID_GAME_RETURN_USEITEM), MB.OKCANCEL, undefined, (id) => {
        if (id === MBID.OK) ui.itemUse.openSelectVillage({ select: onSelect });
      });
    },
    upperBox(tid) {
      deps.ui()?.msgbox.openUpper(text(tid));
    },
    pictureMap() {
      // `CWndMgr::OpenMap`: the picture map window is not part of the item windows (WP-UI map work); the item stays in the bag
    },
    randomScrollConfirm(target: InvItem, hasRandomOption, yes) {
      const name = deps.prop(target.itemId)?.name ?? '';
      const tid = hasRandomOption ? TID_GAME_RANDOMSCROLL_CONFIRM1 : TID_GAME_RANDOMSCROLL_CONFIRM;
      deps.ui()?.itemUse.openRandomScrollConfirm({ text: formatTid(deps.texts, tid, [name]), yes });
    },
    smeltSafetyConfirm(mode, yes, no) {
      const ui = deps.ui();
      if (!ui) return;
      let answered = false;
      ui.itemUse.openSmeltSafetyConfirm({
        text: text(SMELT_SAFETY_TEXT[mode]),
        yes: () => {
          answered = true;
          yes();
        },
        no: () => {
          answered = true;
          no();
        },
        // the window closed by its title bar: the base cursor comes back (`OnDestroy`)
        closed: () => {
          if (!answered) no();
        },
      });
    },
    ...extra,
  };
}

// ---- the summon window's request ---------------------------------------------------------------------------------------------------

/** The Shift key as the summon window reads it (`m_bShiftPushed`): a document listener, `false` without a DOM. */
export function createShiftTracker(): () => boolean {
  let down = false;
  const doc = (globalThis as { document?: Document }).document;
  if (doc) {
    doc.addEventListener('keydown', (e) => {
      if (e.key === 'Shift') down = true;
    });
    doc.addEventListener('keyup', (e) => {
      if (e.key === 'Shift') down = false;
    });
    globalThis.addEventListener('blur', () => {
      down = false;
    });
  }
  return () => down;
}

export interface SummonAngelDeps {
  readonly item: (objId: number) => InvItem | null;
  prop(itemId: number): ClientItem | undefined;
  setExtra(objId: number, extra: number): void;
  shiftDown(): boolean;
  message(tid: number): void;
  inventoryRect(): { l: number; t: number; r: number; b: number } | null;
  create(materials: readonly { objId: number; count: number }[]): void;
}

/** `kind` of a summon material: the two dice, in both of their ids. */
export function angelKind(itemId: number): AngelStack['kind'] {
  if (itemId === II_GEN_MAT_ORICHALCUM01 || itemId === II_GEN_MAT_ORICHALCUM01_1)
    return 'orichalcum';
  if (itemId === II_GEN_MAT_MOONSTONE || itemId === II_GEN_MAT_MOONSTONE_1) return 'moonstone';
  return null;
}

/** The request of `ui.itemUse.openSummonAngel` over the inventory. */
export function createSummonAngelRequest(deps: SummonAngelDeps): SummonAngelRequest {
  const iconOf = (id: number): string | null => {
    const p = deps.prop(id);
    return p && p.icon !== '' ? `item/${p.icon}` : null;
  };
  return {
    stack(objId) {
      const it = deps.item(objId);
      if (!it) return null;
      return {
        objId: it.objId,
        itemId: it.itemId,
        count: it.num,
        extra: it.extra,
        kind: angelKind(it.itemId),
        icon: iconOf(it.itemId),
      };
    },
    setExtra: (objId, extra) => {
      deps.setExtra(objId, extra);
    },
    shiftDown: () => deps.shiftDown(),
    orichalcumIcon: iconOf(II_GEN_MAT_ORICHALCUM01),
    moonstoneIcon: iconOf(II_GEN_MAT_MOONSTONE),
    create: (materials) => {
      deps.create(materials);
    },
    message: (tid) => {
      deps.message(tid);
    },
    inventoryRect: () => deps.inventoryRect(),
  };
}
