// The picture of a class in the fast job change window (`CWndJobChangeEx::OnDraw`, `src:_Interface/WndChangeJobEx.cpp:90-162`): the
// first and second jobs have their own `Slot<Job>.bmp`; the master and hero of a line use the picture of its second job.

import {
  JOB_ACROBAT,
  JOB_ASSIST,
  JOB_BILLPOSTER,
  JOB_BLADE,
  JOB_ELEMENTOR,
  JOB_JESTER,
  JOB_KNIGHT,
  JOB_MAGICIAN,
  JOB_MERCENARY,
  JOB_PSYCHIKEEPER,
  JOB_RANGER,
  JOB_RINGMASTER,
  MAX_PROFESSIONAL,
} from '../../../gen/defs/job';

/** The picture of the error state (`SlotVan.bmp`, the disabled button). */
export const ERROR_PICTURE = 'SlotVan.bmp';

const FIRST: ReadonlyMap<number, string> = new Map([
  [JOB_MERCENARY, 'SlotMer.bmp'],
  [JOB_ACROBAT, 'SlotArc.bmp'],
  [JOB_ASSIST, 'SlotAs.bmp'],
  [JOB_MAGICIAN, 'SlotMag.bmp'],
]);

const LINE: ReadonlyMap<number, string> = new Map([
  [JOB_KNIGHT, 'SlotKnight.bmp'],
  [JOB_BLADE, 'SlotBlad.bmp'],
  [JOB_JESTER, 'SlotJes.bmp'],
  [JOB_RANGER, 'SlotRan.bmp'],
  [JOB_RINGMASTER, 'SlotRingm.bmp'],
  [JOB_BILLPOSTER, 'SlotBillfor.bmp'],
  [JOB_PSYCHIKEEPER, 'SlotPsy.bmp'],
  [JOB_ELEMENTOR, 'SlotElem.bmp'],
]);

/**
 * `Slot<Job>.bmp` of a job; `null` for a job without a picture (the original draws nothing for it). Masters (16..23) and heroes
 * (24..31) map onto the second job of their line (`job - 10` / `job - 18`).
 */
export function jobPicture(job: number): string | null {
  const first = FIRST.get(job);
  if (first !== undefined) return first;
  if (job < MAX_PROFESSIONAL) return LINE.get(job) ?? null;
  if (job < 24) return LINE.get(job - 10) ?? null;
  if (job < 32) return LINE.get(job - 18) ?? null;
  return null;
}
