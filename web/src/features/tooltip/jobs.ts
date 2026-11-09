// Job helpers of the tooltip: the `PutJob` text table and the two job predicates the requirement lines colour by (Plan/45 §10.2
// rows 12-13). The text table is the `switch` of `CWndMgr::PutJob` (`src:_Interface/WndManager.cpp:6125-6240`); the predicates
// mirror `flyff_rules::jobs::{job_type_by_layout, is_interiority_job}` (`MoverParam.cpp:408-460`), which the wasm `RulesContext`
// does not export (the tooltip needs a yes/no per line, not a batch), so they are ported here and pinned by `jobs.test.ts`.

import {
  JOB_ACROBAT,
  JOB_ASSIST,
  JOB_BILLPOSTER,
  JOB_BILLPOSTER_HERO,
  JOB_BILLPOSTER_MASTER,
  JOB_BLADE,
  JOB_BLADE_HERO,
  JOB_BLADE_MASTER,
  JOB_DOPPLER,
  JOB_ELEMENTOR,
  JOB_ELEMENTOR_HERO,
  JOB_ELEMENTOR_MASTER,
  JOB_GATEKEEPER,
  JOB_JESTER,
  JOB_JESTER_HERO,
  JOB_JESTER_MASTER,
  JOB_KNIGHT,
  JOB_KNIGHT_HERO,
  JOB_KNIGHT_MASTER,
  JOB_MAGICIAN,
  JOB_MERCENARY,
  JOB_PSYCHIKEEPER,
  JOB_PSYCHIKEEPER_HERO,
  JOB_PSYCHIKEEPER_MASTER,
  JOB_PUPPETEER,
  JOB_RANGER,
  JOB_RANGER_HERO,
  JOB_RANGER_MASTER,
  JOB_RINGMASTER,
  JOB_RINGMASTER_HERO,
  JOB_RINGMASTER_MASTER,
  JOB_VAGRANT,
  JTYPE_BASE,
  JTYPE_EXPERT,
  JTYPE_HERO,
  JTYPE_MASTER,
  JTYPE_PRO,
  MAX_EXPERT,
  MAX_JOB,
  MAX_MASTER,
  MAX_PROFESSIONAL,
} from '../../gen/defs/job';
import {
  TID_GAME_TOOLTIP_ACRO,
  TID_GAME_TOOLTIP_ASSIST,
  TID_GAME_TOOLTIP_BILLPOS,
  TID_GAME_TOOLTIP_BILLPOSTER_HERO,
  TID_GAME_TOOLTIP_BILLPOSTER_MASTER,
  TID_GAME_TOOLTIP_BLADE,
  TID_GAME_TOOLTIP_BLADE_HERO,
  TID_GAME_TOOLTIP_BLADE_MASTER,
  TID_GAME_TOOLTIP_DOPPLER,
  TID_GAME_TOOLTIP_ELEMENTOR,
  TID_GAME_TOOLTIP_ELEMENTOR_HERO,
  TID_GAME_TOOLTIP_ELEMENTOR_MASTER,
  TID_GAME_TOOLTIP_GATE,
  TID_GAME_TOOLTIP_JASTER,
  TID_GAME_TOOLTIP_JESTER_HERO,
  TID_GAME_TOOLTIP_JESTER_MASTER,
  TID_GAME_TOOLTIP_KNIGHT,
  TID_GAME_TOOLTIP_KNIGHT_HERO,
  TID_GAME_TOOLTIP_KNIGHT_MASTER,
  TID_GAME_TOOLTIP_MAG,
  TID_GAME_TOOLTIP_PSYCHIKEEPER,
  TID_GAME_TOOLTIP_PSYCHIKEEPER_HERO,
  TID_GAME_TOOLTIP_PSYCHIKEEPER_MASTER,
  TID_GAME_TOOLTIP_PUPPET,
  TID_GAME_TOOLTIP_RANGER,
  TID_GAME_TOOLTIP_RANGER_HERO,
  TID_GAME_TOOLTIP_RANGER_MASTER,
  TID_GAME_TOOLTIP_REGMERSER,
  TID_GAME_TOOLTIP_REGVANG,
  TID_GAME_TOOLTIP_RINGMAS,
  TID_GAME_TOOLTIP_RINGMASTER_HERO,
  TID_GAME_TOOLTIP_RINGMASTER_MASTER,
} from '../../gen/defs/text';

/** `PutJob`: the TID of the job restriction text of each job. */
export const JOB_TOOLTIP_TID: ReadonlyMap<number, number> = new Map<number, number>([
  [JOB_VAGRANT, TID_GAME_TOOLTIP_REGVANG],
  [JOB_MERCENARY, TID_GAME_TOOLTIP_REGMERSER],
  [JOB_ACROBAT, TID_GAME_TOOLTIP_ACRO],
  [JOB_ASSIST, TID_GAME_TOOLTIP_ASSIST],
  [JOB_MAGICIAN, TID_GAME_TOOLTIP_MAG],
  [JOB_PUPPETEER, TID_GAME_TOOLTIP_PUPPET],
  [JOB_KNIGHT, TID_GAME_TOOLTIP_KNIGHT],
  [JOB_BLADE, TID_GAME_TOOLTIP_BLADE],
  [JOB_JESTER, TID_GAME_TOOLTIP_JASTER],
  [JOB_RANGER, TID_GAME_TOOLTIP_RANGER],
  [JOB_RINGMASTER, TID_GAME_TOOLTIP_RINGMAS],
  [JOB_BILLPOSTER, TID_GAME_TOOLTIP_BILLPOS],
  [JOB_PSYCHIKEEPER, TID_GAME_TOOLTIP_PSYCHIKEEPER],
  [JOB_ELEMENTOR, TID_GAME_TOOLTIP_ELEMENTOR],
  [JOB_GATEKEEPER, TID_GAME_TOOLTIP_GATE],
  [JOB_DOPPLER, TID_GAME_TOOLTIP_DOPPLER],
  [JOB_KNIGHT_MASTER, TID_GAME_TOOLTIP_KNIGHT_MASTER],
  [JOB_BLADE_MASTER, TID_GAME_TOOLTIP_BLADE_MASTER],
  [JOB_JESTER_MASTER, TID_GAME_TOOLTIP_JESTER_MASTER],
  [JOB_RANGER_MASTER, TID_GAME_TOOLTIP_RANGER_MASTER],
  [JOB_RINGMASTER_MASTER, TID_GAME_TOOLTIP_RINGMASTER_MASTER],
  [JOB_BILLPOSTER_MASTER, TID_GAME_TOOLTIP_BILLPOSTER_MASTER],
  [JOB_PSYCHIKEEPER_MASTER, TID_GAME_TOOLTIP_PSYCHIKEEPER_MASTER],
  [JOB_ELEMENTOR_MASTER, TID_GAME_TOOLTIP_ELEMENTOR_MASTER],
  [JOB_KNIGHT_HERO, TID_GAME_TOOLTIP_KNIGHT_HERO],
  [JOB_BLADE_HERO, TID_GAME_TOOLTIP_BLADE_HERO],
  [JOB_JESTER_HERO, TID_GAME_TOOLTIP_JESTER_HERO],
  [JOB_RANGER_HERO, TID_GAME_TOOLTIP_RANGER_HERO],
  [JOB_RINGMASTER_HERO, TID_GAME_TOOLTIP_RINGMASTER_HERO],
  [JOB_BILLPOSTER_HERO, TID_GAME_TOOLTIP_BILLPOSTER_HERO],
  [JOB_PSYCHIKEEPER_HERO, TID_GAME_TOOLTIP_PSYCHIKEEPER_HERO],
  [JOB_ELEMENTOR_HERO, TID_GAME_TOOLTIP_ELEMENTOR_HERO],
]);

/** The tier of a job by the fixed `defineJob.h` layout (`JTYPE_*`), `undefined` outside 0..31. */
export function jobTypeByLayout(job: number): number | undefined {
  if (job === 0) return JTYPE_BASE;
  if (job >= 1 && job < MAX_EXPERT) return JTYPE_EXPERT;
  if (job >= MAX_EXPERT && job < MAX_PROFESSIONAL) return JTYPE_PRO;
  if (job >= MAX_PROFESSIONAL && job < MAX_MASTER) return JTYPE_MASTER;
  if (job >= MAX_MASTER && job < MAX_JOB) return JTYPE_HERO;
  return undefined;
}

/**
 * `CMover::IsInteriorityJob(item_job)`: may a mover of job `own` use an item restricted to `itemJob`? The vagrant and the own job
 * always; a pro its expert's items; a master those of its expert and pro; a hero those of its expert, pro and master.
 */
export function isInteriorityJob(own: number, itemJob: number): boolean {
  if (itemJob === 0 || itemJob === own) return true;
  const type = jobTypeByLayout(own);
  // an expert's pros are `j * 2 + 4` and `j * 2 + 5`; its masters `j * 2 + 14/15`; its heroes `j * 2 + 22/23`
  const expertLine = (first: number): boolean =>
    itemJob < MAX_EXPERT && (own === itemJob * 2 + first || own === itemJob * 2 + first + 1);
  if (type === JTYPE_PRO && itemJob > 0 && expertLine(4)) return true;
  if (type === JTYPE_MASTER) {
    if (expertLine(14)) return true;
    if (itemJob >= MAX_EXPERT && itemJob < MAX_PROFESSIONAL && itemJob + 10 === own) return true;
  }
  if (type === JTYPE_HERO) {
    if (expertLine(22)) return true;
    if (itemJob >= MAX_EXPERT && itemJob < MAX_PROFESSIONAL && itemJob + 18 === own) return true;
    if (itemJob >= MAX_PROFESSIONAL && itemJob < MAX_MASTER && itemJob + 8 === own) return true;
  }
  return false;
}
