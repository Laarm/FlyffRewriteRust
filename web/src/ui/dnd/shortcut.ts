// The drag payload (Plan/42 §10.1; `SHORTCUT`, `_Common/ProjectCmn.h:977-994`). Only the TYPE lives here: the
// framework signatures (`onDropIcon`, `onChildNotify(WIN_ITEMDROP, ...)`) need it; the drag controller is WP-UI-12.

import type { WndBase } from '../wnd/WndBase';

/** `SHORTCUT_*` kinds. */
export const SC = {
  NONE: 0,
  ETC: 1,
  APPLET: 2,
  MOTION: 3,
  SCRIPT: 4,
  ITEM: 5,
  SKILL: 6,
  OBJECT: 7,
  CHAT: 8,
  SKILLFUN: 9,
  EMOTICON: 10,
  LORDSKILL: 11,
} as const;
export type ScKind = (typeof SC)[keyof typeof SC];

export interface Shortcut {
  /** Wire part = proto `Shortcut` (11 §7.3) = 13 `ShortcutRecord`. */
  kind: ScKind;
  id: number;
  type: number;
  index: number;
  userId: number;
  data: number;
  /** Only for `SC.CHAT`, at most 127 bytes. */
  text?: string;
  // client-only
  /** `m_pFromWnd`: the source control or frame. */
  from?: WndBase;
  /** VFS path of the icon drawn at the cursor. */
  icon?: string;
  /** `m_dwItemId`: the prop id of a stackable item (re-resolved when the object id vanishes). */
  itemId?: number;
  srcSlot?: { row: 'applet' | 'item' | 'queue'; page?: number; slot: number };
}
