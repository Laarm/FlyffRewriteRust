// Sections 12-14 of the item tooltip: the job, the level and the description (Plan/45 §10.2; `PutJob`, `PutLevel`, `PutCommand`;
// `src:_Interface/WndManager.cpp:6125-6272`).

import {
  II_GEN_WARP_COUPLERING,
  II_GEN_WARP_COUPLERING01,
  II_GEN_WARP_WEDDING_BAND,
} from '../../../gen/defs/item';
import { JTYPE_MASTER } from '../../../gen/defs/job';
import {
  TID_GAME_REQLEVEL,
  TID_GAME_TOOLTIP_REQLEVEL,
  TID_ITEM_COUPLERING_DESC,
  TID_ITEM_INFO,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR } from '../../../ui/text/markup';
import { ITEM_COUNT_COLOR } from '../colors';
import { NULL_ID } from '../../inventory/info';
import { JOB_TOOLTIP_TID, jobTypeByLayout } from '../jobs';
import type { ComposeCx } from '../model';
import { sprintf } from '../printf';

/** `PutJob`: the job restriction, in the not-usable colour unless the viewer's job may use it. */
export function putJob(cx: ComposeCx): void {
  const { prop, env, b, colors, owner } = cx;
  if (prop.item_job === NULL_ID) return;
  const tid = JOB_TOOLTIP_TID.get(prop.item_job);
  const line = tid === undefined ? '' : sprintf(env.text(tid));
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(line, env.isInteriorityJob(owner.job, prop.item_job) ? colors.general : colors.notUse);
}

/** `CItemElem::GetLevelDown()` of the 64-bit random-option word: bit 63 = -10, bit 62 = -5. */
export function levelDown(word: bigint): number {
  const w = BigInt.asUintN(64, word);
  if ((w & 0x8000_0000_0000_0000n) !== 0n) return -10;
  if ((w & 0x4000_0000_0000_0000n) !== 0n) return -5;
  return 0;
}

/** `CItemElem::IsLimitLevel(mover)` (`__NEW_ITEM_LIMIT_LEVEL`): a master or hero wearing an item of a lower tier is never limited. */
export function isLimitLevel(cx: ComposeCx): boolean {
  const { prop, item, owner } = cx;
  const own = owner.jobType;
  const itemTier = prop.item_job === NULL_ID ? own : jobTypeByLayout(prop.item_job);
  if (own >= JTYPE_MASTER && itemTier !== undefined && own > itemTier) return false;
  if (prop.limit_level1 === NULL_ID) return false;
  const limit = Math.max(0, prop.limit_level1 + levelDown(item.randomOpt));
  return owner.level < limit;
}

/** `PutLevel`: the required level (red when too low) and the awakening level-down `(N)`. */
export function putLevel(cx: ComposeCx): void {
  const { prop, item, env, b, colors, numbers } = cx;
  if (prop.limit_level1 === NULL_ID) return;
  b.add('\n', DEFAULT_TEXT_COLOR);
  const limited = numbers?.equipDenied === TID_GAME_REQLEVEL || isLimitLevel(cx);
  b.add(
    sprintf(env.text(TID_GAME_TOOLTIP_REQLEVEL), [prop.limit_level1]),
    limited ? colors.notUse : colors.general,
  );
  const down = levelDown(item.randomOpt);
  if (down !== 0) b.add(`(${String(down)})`, colors.notUse);
}

/** `PutCommand`: the quantity line (`item_count_tooltip`) and the description, with the couple ring's partner name. */
export function putCommand(cx: ComposeCx): void {
  const { prop, item, env, b, colors } = cx;
  if (env.features().item_count_tooltip && prop.pack_max > 1) {
    b.add(`\nQuantity: ${String(item.num)}`, ITEM_COUNT_COLOR);
  }
  if (prop.description === '') return;
  b.add('\n', DEFAULT_TEXT_COLOR);
  let line: string;
  const ring =
    prop.id === II_GEN_WARP_COUPLERING ||
    prop.id === II_GEN_WARP_WEDDING_BAND ||
    prop.id === II_GEN_WARP_COUPLERING01;
  const partner = Number(BigInt.asUintN(32, item.randomOpt));
  if (ring && item.randomOpt > 0n) {
    const desc = sprintf(env.text(TID_ITEM_COUPLERING_DESC), [env.playerName(partner) ?? '']);
    line = sprintf(env.text(TID_ITEM_INFO), [desc]);
  } else {
    line = sprintf(env.text(TID_ITEM_INFO), [prop.description]);
  }
  b.add(line, colors.command);
}
