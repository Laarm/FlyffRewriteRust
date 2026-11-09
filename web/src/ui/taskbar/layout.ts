// The taskbar layout choice and the slot geometry (Plan/42 §11.1; WndTaskBar.cpp:1174-1338; PL-124).
//
// The taskbar is the `APP_TASKBAR<w>` applet with the largest legacy width `w` that fits the virtual width; below 800
// the 800 layout is used (FIX, PL-124: the original created no taskbar for non-standard widths). The applet row has 5,
// 10 or 18 slots (`m_nMaxSlotApplet`).

import {
  APP_TASKBAR1024,
  APP_TASKBAR1280,
  APP_TASKBAR1360W,
  APP_TASKBAR1400,
  APP_TASKBAR1440W,
  APP_TASKBAR1600,
  APP_TASKBAR1680W,
  APP_TASKBAR800,
} from '../../gen/defs/resdata';

/** Height of the taskbar in UI virtual pixels. */
export const TASKBAR_H = 48;
/** Applet and item cells are 32 x 32; the action queue cells are 28 wide. */
export const SLOT_SIZE = 32;
export const QUEUE_SLOT_W = 28;
export const QUEUE_SLOT_H = 32;
/** Maxima of `TaskBar` (11 §11.6). */
export const MAX_SLOT_APPLET = 18;
export const MAX_SLOT_ITEM_COUNT = 8;
export const MAX_SLOT_ITEM = 9;
export const MAX_SLOT_QUEUE = 5;
/** The action points the chain pays before the skill in queue slot `i` (`UserTaskBar::SetNextSkill`: 6, 8, 11, 30; slot 0 is free). */
export const SKILL_QUEUE_AP_COSTS: readonly number[] = [0, 6, 8, 11, 30];
/** The action-skill icon sits 37 px left of the queue origin. */
export const ACTION_ICON_DX = 37;

export interface TaskbarLayoutSpec {
  /** The `APP_TASKBAR*` id of the layout. */
  readonly app: number;
  /** The layout's name (`APP_TASKBAR1440W`). */
  readonly name: string;
  /** The legacy width of the layout (800, 1024, ... 1680). */
  readonly legacyWidth: number;
  /** Applet slots (`m_nMaxSlotApplet`). */
  readonly appletSlots: number;
}

const LAYOUT_800: TaskbarLayoutSpec = {
  app: APP_TASKBAR800,
  name: 'APP_TASKBAR800',
  legacyWidth: 800,
  appletSlots: 5,
};

/** The eight layouts in width order. */
export const TASKBAR_LAYOUTS: readonly TaskbarLayoutSpec[] = [
  LAYOUT_800,
  { app: APP_TASKBAR1024, name: 'APP_TASKBAR1024', legacyWidth: 1024, appletSlots: 10 },
  { app: APP_TASKBAR1280, name: 'APP_TASKBAR1280', legacyWidth: 1280, appletSlots: 18 },
  { app: APP_TASKBAR1360W, name: 'APP_TASKBAR1360W', legacyWidth: 1360, appletSlots: 18 },
  { app: APP_TASKBAR1400, name: 'APP_TASKBAR1400', legacyWidth: 1400, appletSlots: 18 },
  { app: APP_TASKBAR1440W, name: 'APP_TASKBAR1440W', legacyWidth: 1440, appletSlots: 18 },
  { app: APP_TASKBAR1600, name: 'APP_TASKBAR1600', legacyWidth: 1600, appletSlots: 18 },
  { app: APP_TASKBAR1680W, name: 'APP_TASKBAR1680W', legacyWidth: 1680, appletSlots: 18 },
];

/** The layout for a virtual width: the widest one that is `<=` the width, the 800 layout below 800. */
export function chooseTaskbarLayout(virtualWidth: number): TaskbarLayoutSpec {
  let best = LAYOUT_800;
  for (const l of TASKBAR_LAYOUTS) {
    if (l.legacyWidth <= virtualWidth) best = l;
  }
  return best;
}

export interface SlotRect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** Which row of the taskbar a point is in, and the slot index. */
export type SlotHit =
  { row: 'applet'; slot: number } | { row: 'item'; slot: number } | { row: 'queue'; slot: number };

/** Cell `i` of a row starting at `origin` (cells of `size.w` x `size.h`). */
export function cellRect(
  origin: { x: number; y: number },
  size: { w: number; h: number },
  i: number,
): SlotRect {
  return { x: origin.x + i * size.w, y: origin.y, w: size.w, h: size.h };
}

/**
 * The slot under a point (client coordinates of the taskbar frame): the applet row (`appletSlots` cells at
 * `applets`), the item row (9 cells at `items`) and the queue (5 cells of 28 px at `queue`).
 */
export function hitSlot(
  pt: { x: number; y: number },
  origins: {
    applets: { x: number; y: number };
    items: { x: number; y: number };
    queue: { x: number; y: number };
  },
  appletSlots: number,
): SlotHit | null {
  const inRow = (o: { x: number; y: number }, w: number, h: number, n: number): number => {
    if (pt.y < o.y || pt.y >= o.y + h || pt.x < o.x || pt.x >= o.x + n * w) return -1;
    return Math.floor((pt.x - o.x) / w);
  };
  const a = inRow(origins.applets, SLOT_SIZE, SLOT_SIZE, appletSlots);
  if (a >= 0) return { row: 'applet', slot: a };
  const i = inRow(origins.items, SLOT_SIZE, SLOT_SIZE, MAX_SLOT_ITEM);
  if (i >= 0) return { row: 'item', slot: i };
  const q = inRow(origins.queue, QUEUE_SLOT_W, QUEUE_SLOT_H, MAX_SLOT_QUEUE);
  if (q >= 0) return { row: 'queue', slot: q };
  return null;
}
