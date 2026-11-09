// The clicks on the HUD buff icons (Plan/43 §4.4, §4.5, §7.13; WP-INPUT-12; `CWndWorld::OnLButtonDown`, `OnLButtonDblClk`, `WndWorld.cpp:6975-7000`,
// `7470-7495`). The icons belong to the 45 buff strip; `InputHooks.buffIconAt(x, y)` names the item buff under the pointer (`GetBuffIconRect`):
//
//   single click    the return scroll, the party summon and the pet feed pocket icons wait for the double click: the click is swallowed
//                   (no selection, no destination); the four angel icons (red, green, blue, white) send `AngelBuff`
//                   (the 100 % angel item of a finished angel buff) and swallow the click as well;
//   double click    return scroll -> `ReturnScrollReq{select: -1}` (back to the saved position), pet feed pocket -> `FeedPocketInactive`,
//                   party summon -> `hooks.partySummon()` (45 opens the use window).
//
// The strip window of 42 claims the pointer over its icons itself and calls the same messages through `BuffActions` (`features/buffs`); this
// module is the world window's half, and the one place that knows which icon does what.

import { create } from '@bufbuild/protobuf';
import {
  II_SYS_SYS_QUE_ANGEL_BLUE,
  II_SYS_SYS_QUE_ANGEL_GREEN,
  II_SYS_SYS_QUE_ANGEL_RED,
  II_SYS_SYS_QUE_ANGEL_WHITE,
  II_SYS_SYS_SCR_PARTYSUMMON,
  II_SYS_SYS_SCR_PET_FEED_POCKET,
  II_SYS_SYS_SCR_RETURN,
} from '../gen/defs/item';
import { ReturnScrollReqSchema } from '../gen/proto/flyff/v1/inventory_pb';
import { AngelBuffSchema, FeedPocketInactiveSchema } from '../gen/proto/flyff/v1/pet_pb';
import type { ClientMsg } from '../net/codec';

/** The four angel buff icons (the 100 % variants are not clickable in the original). */
export const ANGEL_ICONS: readonly number[] = [
  II_SYS_SYS_QUE_ANGEL_RED,
  II_SYS_SYS_QUE_ANGEL_GREEN,
  II_SYS_SYS_QUE_ANGEL_BLUE,
  II_SYS_SYS_QUE_ANGEL_WHITE,
];

/** The icons whose single click only waits for the double click. */
export const DOUBLE_CLICK_ICONS: readonly number[] = [
  II_SYS_SYS_SCR_RETURN,
  II_SYS_SYS_SCR_PARTYSUMMON,
  II_SYS_SYS_SCR_PET_FEED_POCKET,
];

export const isAngelIcon = (icon: number): boolean => ANGEL_ICONS.includes(icon);

/** `SendAngelBuff`: redeem the finished angel buff for a 100 % angel item. */
export function angelBuffMsg(): ClientMsg {
  // legacy-send: SendAngelBuff
  return { case: 'angelBuff', value: create(AngelBuffSchema, {}) };
}

/** What a single click on `icon` does in the world window: `true` = the click is taken (nothing is selected, no destination). */
export function buffIconClick(icon: number | null, send: (msg: ClientMsg) => void): boolean {
  if (icon === null) return false;
  if (DOUBLE_CLICK_ICONS.includes(icon)) return true; // wait for the double click
  if (isAngelIcon(icon)) {
    send(angelBuffMsg());
    return true;
  }
  return false;
}

/** What the double click on `icon` sends (`II_SYS_SYS_SCR_PARTYSUMMON` goes to the hook). */
export function buffIconDoubleClick(
  icon: number | null,
  send: (msg: ClientMsg) => void,
  partySummon: () => void,
): void {
  if (icon === II_SYS_SYS_SCR_RETURN) {
    // legacy-send: SendReturnScroll
    send({ case: 'returnScrollReq', value: create(ReturnScrollReqSchema, { select: -1 }) });
  } else if (icon === II_SYS_SYS_SCR_PARTYSUMMON) {
    partySummon();
  } else if (icon === II_SYS_SYS_SCR_PET_FEED_POCKET) {
    // legacy-send: SendFeedPocketInactive
    send({ case: 'feedPocketInactive', value: create(FeedPocketInactiveSchema, {}) });
  }
}
