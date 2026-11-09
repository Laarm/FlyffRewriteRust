// The numbers behind the action-queue display (`CWndTaskBar::OnDraw`, WndTaskBar.cpp:1055-1105; Plan/42 §11.8, 45 §19.3): how many cells
// the action points pay for, how much of the cell row is shaded, and what the next cell costs. Pure functions.

import { SKILL_QUEUE_AP_COSTS } from './layout';

/** `LV2MAXAP`, `LV3MAXAP`, `LV4MAXAP`, `LV5MAXAP`: the points that light the 2nd, 3rd, 4th and 5th cell. */
export const AP_LEVELS = [10, 25, 50, 100] as const;

/**
 * The width of the lit part of the shaded area behind the first cell, in pixels: the cells 2..5 are `4 * cell` wide and fill up with
 * the points (`nAP2Size`): 0..10 points fill the 2nd cell, 10..25 the 3rd, 25..50 the 4th, 50..100 the 5th. `dark` is what stays shaded.
 */
export function actionPointShade(ap: number, cell: number): { lit: number; dark: number } {
  const p = Math.max(0, Math.trunc(ap));
  const [l2, l3, l4, l5] = AP_LEVELS;
  let lit: number;
  if (p >= l4) lit = cell * 3 + Math.trunc(((p - l4) * cell) / (l5 - l4));
  else if (p >= l3) lit = cell * 2 + Math.trunc(((p - l3) * cell) / (l4 - l3));
  else if (p >= l2) lit = cell + Math.trunc(((p - l2) * cell) / (l3 - l2));
  else lit = Math.trunc((p * cell) / l2);
  const total = cell * 4;
  lit = Math.min(lit, total);
  return { lit, dark: total - lit };
}

/** `show` of the lamp: the number of cells (1..5) the points pay for, but never more than the queue holds; `0` for an empty queue. */
export function lampSlots(ap: number, queueCount: number): number {
  if (queueCount <= 0) return 0;
  const p = Math.trunc(ap);
  let show = 1;
  if (p >= AP_LEVELS[0]) show = 2;
  if (p >= AP_LEVELS[1]) show = 3;
  if (p >= AP_LEVELS[2]) show = 4;
  if (p >= AP_LEVELS[3]) show = 5;
  return Math.min(show, queueCount);
}

/** The points the skill in queue slot `slot` costs when the chain reaches it (`{1: 6, 2: 8, 3: 11, 4: 30}`, the first one is free). */
export function queueApCost(slot: number): number {
  return SKILL_QUEUE_AP_COSTS[slot] ?? 0;
}
