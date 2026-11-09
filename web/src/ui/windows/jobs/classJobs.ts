// The radio tables of the two class-change windows and the job -> window rule, shared by the lazy shells and the service (`ui/wnd/lazy.ts`).

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
} from '../../../gen/defs/job';
import type { WidcKey } from '../../layout/registry';

export interface Radio {
  readonly name: WidcKey;
  readonly job: number;
}

/** `WIDC_RADIO_*` of the layouts in the order of the original's `bJob[]` (the job id minus the first job of the window). */
export const CLASS1: readonly Radio[] = [
  { name: 'WIDC_RADIO_MER', job: JOB_MERCENARY },
  { name: 'WIDC_RADIO_ACR', job: JOB_ACROBAT },
  { name: 'WIDC_RADIO_ASS', job: JOB_ASSIST },
  { name: 'WIDC_RADIO_MAG', job: JOB_MAGICIAN },
];
export const CLASS2: readonly Radio[] = [
  { name: 'WIDC_RADIO_KNI', job: JOB_KNIGHT },
  { name: 'WIDC_RADIO_BLA', job: JOB_BLADE },
  { name: 'WIDC_RADIO_JES', job: JOB_JESTER },
  { name: 'WIDC_RADIO_RAN', job: JOB_RANGER },
  { name: 'WIDC_RADIO_RIN', job: JOB_RINGMASTER },
  { name: 'WIDC_RADIO_BIL', job: JOB_BILLPOSTER },
  { name: 'WIDC_RADIO_PSY', job: JOB_PSYCHIKEEPER },
  { name: 'WIDC_RADIO_ELE', job: JOB_ELEMENTOR },
];

/** Does a window exist for this job? (The original indexes its radio table by the job: other jobs crash it.) */
export function changeClassWindowFor(job: number): 1 | 2 | null {
  if (CLASS1.some((r) => r.job === job)) return 1;
  if (CLASS2.some((r) => r.job === job)) return 2;
  return null;
}
