// `CWndTaskBar::CheckAddSkill(styleSrc, dest)` (`_Interface/WndTaskBar.cpp:1712-1830`; Plan/45 §19.3, 42 §11.8; WP-CFEAT-32): may a skill of
// combo style `src` go into queue slot `dest`? The combo styles are `CT_STEP`, `CT_CIRCLE`, `CT_FINISH` and `CT_GENERAL`; a slot without
// a skill has style 0.
//
//   slot 0         empty queue: STEP or GENERAL; otherwise only a GENERAL may replace a GENERAL
//   slot 4         not STEP; not after a FINISH; CIRCLE and FINISH not after a GENERAL; GENERAL only after a GENERAL
//   slots 1..3     not STEP; not after a FINISH; CIRCLE and FINISH not after a GENERAL; FINISH not onto a CIRCLE or a FINISH;
//                  GENERAL only after a GENERAL, and onto a non-GENERAL only at the end of the queue

import { CT_CIRCLE, CT_FINISH, CT_GENERAL, CT_STEP } from '../../../gen/defs/attribute';

/** The number of queue slots. */
export const QUEUE_SLOTS = 5;

/**
 * @param src       combo style of the skill to put in
 * @param dest      queue slot (0..4)
 * @param styles    combo style of the skill in each queue slot (0 = empty)
 * @param count     number of queued skills (`m_nCurQueueNum`)
 */
export function checkAddSkill(
  src: number,
  dest: number,
  styles: readonly number[],
  count: number,
): boolean {
  const at = (i: number): number => styles[i] ?? 0;
  if (dest === 0) {
    if (count === 0) return src === CT_STEP || src === CT_GENERAL;
    return src === CT_GENERAL && at(0) === CT_GENERAL;
  }
  const prev = at(dest - 1);
  if (src === CT_STEP) return false; // a STEP is only the first skill
  if (prev === CT_FINISH) return false; // nothing follows a FINISH
  if (src === CT_CIRCLE && prev === CT_GENERAL) return false;
  if (src === CT_FINISH && prev === CT_GENERAL) return false;
  if (dest === QUEUE_SLOTS - 1) {
    // the last slot
    return !(src === CT_GENERAL && prev !== CT_GENERAL);
  }
  const cur = at(dest);
  if (src === CT_FINISH && (cur === CT_CIRCLE || cur === CT_FINISH)) return false;
  if (src === CT_GENERAL) {
    if (prev !== CT_GENERAL) return false;
    if (cur !== CT_GENERAL && count > dest) return false;
  }
  return true;
}
