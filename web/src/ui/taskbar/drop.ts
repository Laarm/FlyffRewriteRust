// The drop rules of the taskbar (`CWndTaskBar::OnDropIcon`, WndTaskBar.cpp:1918-2133 and `SetShortcut` :1551-1710;
// Plan/42 §11.6 "client list", ref/22 §12.2; WP-UI-19): a pure function from a dragged shortcut and the slot under the
// pointer to a decision. Applying it (model, messages, the source slot) is the window's.
//
//   1. the source must be the inventory, the chat (macro icon), the skill window, the motion window, the party window,
//      the lord-skill window, the taskbar itself or a start-menu item, else the forbid icon;
//   2. a normal skill needs level > 0 and `CheckSkill`;
//   3. a shortcut without an id and without a text (except SKILL, SKILLFUN and LORDSKILL) is refused;
//   4. by row: applet row and item row accept every kind, but SKILL only with combo style STEP or GENERAL; the item row
//      sanitises chat macros (`--`, `'`, `/*`, `*/` removed); more than ten chat macros (item rows + applet row, client
//      count) are refused with `TID_GAME_MAX_SHORTCUT_CHAT`; the queue accepts normal skills only, through the queue port;
//   5. dropping a slot on its own slot does nothing; a slot dropped on a part of the bar that is no slot is removed.

import {
  APP_COMMUNICATION_CHAT,
  APP_INVENTORY,
  APP_LORD_SKILL,
  APP_MOTION,
  APP_PARTY,
  APP_SKILL3,
} from '../../gen/defs/resdata';
import { CT_GENERAL, CT_STEP } from '../../gen/defs/attribute';
import { TID_GAME_MAX_SHORTCUT_CHAT } from '../../gen/defs/text';
import { SC, type Shortcut } from '../dnd/shortcut';
import type { SkillDropInfo, TaskbarItemPort } from './ports';

export type DropRow = 'applet' | 'item' | 'queue';

/** The slot under the pointer. */
export interface DropTarget {
  readonly row: DropRow;
  readonly slot: number;
}

/** The windows a shortcut may come from (besides the taskbar and the start menu). */
export const DROP_SOURCES: readonly number[] = [
  APP_INVENTORY,
  APP_COMMUNICATION_CHAT,
  APP_SKILL3,
  APP_MOTION,
  APP_PARTY,
  APP_LORD_SKILL,
];

/** Why a drop was refused (the tests and the probe read it). */
export type ForbidReason = 'source' | 'skill' | 'empty' | 'comboStyle' | 'noSlot' | 'queue';

export interface DropEnv {
  /** The APP id of the frame the shortcut was picked up in (`null` = unknown). */
  readonly fromApp: number | null;
  /** Picked up from this taskbar (`m_pFromWnd == this`). */
  readonly fromSelf: boolean;
  /** Picked up from a start-menu item (a menu button below the taskbar). */
  readonly fromMenu: boolean;
  readonly skill: (type: number, id: number) => SkillDropInfo | null;
  readonly items: TaskbarItemPort | null;
  /** The chat macros in all item rows and the applet row (the client count of `SetShortcut`). */
  readonly chatCount: number;
  /** The page of the item row being dropped on (`m_nSlotIndex`). */
  readonly page: number;
}

export type DropDecision =
  | { readonly kind: 'forbid'; readonly why: ForbidReason }
  /** Nothing happens and no icon shows (a slot dropped on itself). */
  | { readonly kind: 'ignore' }
  /** A system-chat line; nothing else (the chat-macro limit). */
  | { readonly kind: 'message'; readonly tid: number }
  /** Put `shortcut` into the applet or item row (and remove the source slot when `fromSelf`). */
  | {
      readonly kind: 'set';
      readonly row: 'applet' | 'item';
      readonly slot: number;
      readonly shortcut: Shortcut;
    }
  /** A normal skill on the queue row: the queue port decides. */
  | { readonly kind: 'queue'; readonly slot: number; readonly shortcut: Shortcut }
  /** A slot of the bar dropped on a part of the bar that is not a slot: the source slot is removed. */
  | { readonly kind: 'removeSource' };

/** `sChat.Replace("--", "")` ... of the item row. */
export function sanitizeMacro(text: string): string {
  return text.split('--').join('').split("'").join('').split('/*').join('').split('*/').join('');
}

function comboOk(combo: number): boolean {
  return combo === CT_STEP || combo === CT_GENERAL;
}

/**
 * The shortcut stored in a slot (`SetShortcut`): `index` = the slot, `userId` 0, `data` 0 for the applet row and 1 for the
 * item row, the text only for CHAT, the LORDSKILL id minus one (KEEP-QUIRK: the window's ids are 1-based), the item id
 * of a stackable item (`m_dwItemId`).
 */
export function slotShortcut(
  sc: Shortcut,
  row: 'applet' | 'item',
  slot: number,
  items: TaskbarItemPort | null,
): Shortcut {
  const out: Shortcut = {
    kind: sc.kind,
    id: sc.kind === SC.LORDSKILL ? sc.id - 1 : sc.id,
    type: sc.type,
    index: slot,
    userId: 0,
    data: row === 'applet' ? 0 : 1,
  };
  if (sc.kind === SC.CHAT) {
    out.text = row === 'item' ? sanitizeMacro(sc.text ?? '') : (sc.text ?? '');
  }
  if (sc.icon !== undefined) out.icon = sc.icon;
  if (sc.kind === SC.ITEM && items) {
    const itemId = items.stackItemId(sc.id);
    if (itemId !== null) out.itemId = itemId;
  }
  return out;
}

/** The shortcut a drag from a slot carries (`m_GlobalShortcut = *slot` + the lord-skill `m_dwId++`). */
export function pickupShortcut(slot: Shortcut): Shortcut {
  return { ...slot, id: slot.kind === SC.LORDSKILL ? slot.id + 1 : slot.id };
}

export function decideDrop(sc: Shortcut, target: DropTarget | null, env: DropEnv): DropDecision {
  /** The slot the shortcut came from when `fromSelf` (to ignore a drop on itself). */
  const source = sc.srcSlot ?? null;
  if (
    !env.fromSelf &&
    !env.fromMenu &&
    (env.fromApp === null || !DROP_SOURCES.includes(env.fromApp))
  ) {
    return { kind: 'forbid', why: 'source' };
  }
  if (sc.kind === SC.SKILL && sc.type !== 2) {
    const info = env.skill(0, sc.id);
    if (!info || info.level <= 0 || !info.usable) return { kind: 'forbid', why: 'skill' };
  }
  // `m_dwId == 0 && strlen(m_szString) < 1`: every skill source sets `m_szString` to the skill name (`CWndSkillTreeEx::OnMouseMove`,
  // `CWndParty`), so the rule never fires for a SKILL - the first slot of a job line (id 0) is a valid skill. Our payloads carry no
  // name, hence the exception here.
  if (
    sc.id === 0 &&
    (sc.text ?? '') === '' &&
    sc.kind !== SC.SKILL &&
    sc.kind !== SC.SKILLFUN &&
    sc.kind !== SC.LORDSKILL
  ) {
    return { kind: 'forbid', why: 'empty' };
  }
  if (!target) return env.fromSelf ? { kind: 'removeSource' } : { kind: 'forbid', why: 'noSlot' };

  const sameSlot =
    env.fromSelf &&
    source !== null &&
    source.row === target.row &&
    source.slot === target.slot &&
    (target.row !== 'item' || (source.page ?? env.page) === env.page);

  if (target.row === 'queue') {
    if (sc.kind === SC.SKILL && sc.type === 0) {
      return { kind: 'queue', slot: target.slot, shortcut: { ...sc, index: target.slot, data: 2 } };
    }
    // KEEP-QUIRK: another kind dropped on the queue from the bar itself deletes the source (no forbid for `fromSelf`)
    return env.fromSelf ? { kind: 'removeSource' } : { kind: 'forbid', why: 'queue' };
  }

  if (sameSlot) return { kind: 'ignore' };
  if (sc.kind === SC.SKILL) {
    const info = env.skill(sc.type === 2 ? 2 : 0, sc.id);
    if (!info || !comboOk(info.combo)) return { kind: 'forbid', why: 'comboStyle' };
  }
  if (sc.kind === SC.CHAT && env.chatCount > 9) {
    // FIX: the original refused the macro but still deleted the source slot of a move (the macro was lost)
    return { kind: 'message', tid: TID_GAME_MAX_SHORTCUT_CHAT };
  }
  return {
    kind: 'set',
    row: target.row,
    slot: target.slot,
    shortcut: slotShortcut(sc, target.row, target.slot, env.items),
  };
}
