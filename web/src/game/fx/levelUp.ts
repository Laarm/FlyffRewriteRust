// The level-up effect (Plan/41 section 17.3, DPClient.cpp:3340-3430 `OnSetLevel`, MoverParam.cpp:1470-1738): with the private-server
// switch `features.v21_levelup_sfx` another player's level-up shows the hero mark of his job line, else (and for the own
// character's `SetExperience` / `SetFxp`) the classic `XI_GEN_LEVEL_UP01`.
//
//   mercenary line  XI_SYS_HEROMARK01      magician line  XI_SYS_HEROMARK05      assist line  XI_SYS_HEROMARK02
//   acrobat line    XI_SYS_HEROMARK06      anything else (vagrant, puppeteer ...)  XI_SYS_HEROMARK03
//
// The line test is `CMover::IsInteriorityJob(line)` (MoverParam.cpp:408-454): the job is the line's expert job, or one of its
// professions (`line * 2 + 4`, `+ 5`), masters (`line * 2 + 14`, `+ 15`) or heroes (`line * 2 + 22`, `+ 23`). The checks run in
// the order mercenary, magician, assist, acrobat.

import {
  XI_GEN_LEVEL_UP01,
  XI_SYS_HEROMARK01,
  XI_SYS_HEROMARK02,
  XI_SYS_HEROMARK03,
  XI_SYS_HEROMARK05,
  XI_SYS_HEROMARK06,
} from '../../gen/defs/obj';
import {
  JOB_ACROBAT,
  JOB_ASSIST,
  JOB_MAGICIAN,
  JOB_MERCENARY,
  JOB_VAGRANT,
  MAX_EXPERT,
  MAX_HERO,
  MAX_MASTER,
  MAX_PROFESSIONAL,
} from '../../gen/defs/job';

/** `IsInteriorityJob(line)` for an expert job `line` (1..4). */
export function inJobLine(job: number, line: number): boolean {
  if (line === JOB_VAGRANT || job === line) return true;
  if (job >= MAX_EXPERT && job < MAX_PROFESSIONAL)
    return line * 2 + 4 === job || line * 2 + 5 === job;
  if (job >= MAX_PROFESSIONAL && job < MAX_MASTER)
    return line * 2 + 14 === job || line * 2 + 15 === job;
  if (job >= MAX_MASTER && job < MAX_HERO) return line * 2 + 22 === job || line * 2 + 23 === job;
  return false;
}

/** The effect of another player's level-up (`OnSetLevel`): the hero mark of the job line, `XI_GEN_LEVEL_UP01` when the switch is off. */
export function levelUpEffect(job: number, v21: boolean): number {
  if (!v21) return XI_GEN_LEVEL_UP01;
  if (inJobLine(job, JOB_MERCENARY)) return XI_SYS_HEROMARK01;
  if (inJobLine(job, JOB_MAGICIAN)) return XI_SYS_HEROMARK05;
  if (inJobLine(job, JOB_ASSIST)) return XI_SYS_HEROMARK02;
  if (inJobLine(job, JOB_ACROBAT)) return XI_SYS_HEROMARK06;
  return XI_SYS_HEROMARK03;
}
