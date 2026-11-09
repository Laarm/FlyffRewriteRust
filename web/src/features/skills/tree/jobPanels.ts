// What the skill tree shows for each job (`CWndSkillTreeEx::InitItem`, `src:_Interface/WndField.cpp:7091-7330`): the captions of the three tier
// headers (the job names of the vagrant, the first and the second job of the line), the pictures under the expert and professional
// sections and, for heroes, the picture under the hero skill. Masters and heroes show the tree of their professional job.

import {
  JOB_ACROBAT,
  JOB_ASSIST,
  JOB_BILLPOSTER,
  JOB_BILLPOSTER_HERO,
  JOB_BILLPOSTER_MASTER,
  JOB_BLADE,
  JOB_BLADE_HERO,
  JOB_BLADE_MASTER,
  JOB_ELEMENTOR,
  JOB_ELEMENTOR_HERO,
  JOB_ELEMENTOR_MASTER,
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
  JOB_RANGER,
  JOB_RANGER_HERO,
  JOB_RANGER_MASTER,
  JOB_RINGMASTER,
  JOB_RINGMASTER_HERO,
  JOB_RINGMASTER_MASTER,
  JOB_VAGRANT,
} from '../../../gen/defs/job';

/** The job ids whose names head the three tiers (`-1` = an empty caption). */
export interface JobPanel {
  readonly titles: readonly [number, number, number];
  /** `Back_*.TGA` under the expert and the professional section. */
  readonly panels: readonly [string | null, string | null];
  /** `Back_Hero_*.tga` under the hero skill (hero jobs only). */
  readonly hero: string | null;
}

const NONE = -1;

function panel(
  titles: readonly [number, number, number],
  panels: readonly [string | null, string | null],
  hero: string | null = null,
): JobPanel {
  return { titles, panels, hero };
}

const PANELS = new Map<number, JobPanel>([
  [JOB_VAGRANT, panel([JOB_VAGRANT, NONE, NONE], [null, null])],
  [JOB_MERCENARY, panel([JOB_VAGRANT, JOB_MERCENARY, NONE], ['Back_Me.TGA', null])],
  [JOB_ASSIST, panel([JOB_VAGRANT, JOB_ASSIST, NONE], ['Back_As.TGA', null])],
  [JOB_MAGICIAN, panel([JOB_VAGRANT, JOB_MAGICIAN, NONE], ['Back_Ma.TGA', null])],
  [JOB_ACROBAT, panel([JOB_VAGRANT, JOB_ACROBAT, NONE], ['Back_Acr.tga', null])],
]);

function line(
  jobs: readonly [number, number, number],
  titles: readonly [number, number, number],
  panels: readonly [string, string],
  hero: string,
): void {
  const [pro, master, heroJob] = jobs;
  PANELS.set(pro, panel(titles, panels));
  PANELS.set(master, panel(titles, panels));
  PANELS.set(heroJob, panel(titles, panels, hero));
}

line(
  [JOB_KNIGHT, JOB_KNIGHT_MASTER, JOB_KNIGHT_HERO],
  [JOB_VAGRANT, JOB_MERCENARY, JOB_KNIGHT],
  ['Back_Me.TGA', 'Back_Night.TGA'],
  'Back_Hero_KntDrawing.tga',
);
line(
  [JOB_BLADE, JOB_BLADE_MASTER, JOB_BLADE_HERO],
  [JOB_VAGRANT, JOB_MERCENARY, JOB_BLADE],
  ['Back_Me.TGA', 'Back_Blade.TGA'],
  'Back_Hero_BldDefence.tga',
);
line(
  [JOB_BILLPOSTER, JOB_BILLPOSTER_MASTER, JOB_BILLPOSTER_HERO],
  [JOB_VAGRANT, JOB_ASSIST, JOB_BILLPOSTER],
  ['Back_As.TGA', 'Back_Bill.TGA'],
  'Back_Hero_BilDisEnchant.tga',
);
line(
  [JOB_RINGMASTER, JOB_RINGMASTER_MASTER, JOB_RINGMASTER_HERO],
  [JOB_VAGRANT, JOB_ASSIST, JOB_RINGMASTER],
  ['Back_As.TGA', 'Back_Ring.TGA'],
  'Back_Hero_RigReturn.tga',
);
line(
  [JOB_ELEMENTOR, JOB_ELEMENTOR_MASTER, JOB_ELEMENTOR_HERO],
  [JOB_VAGRANT, JOB_MAGICIAN, JOB_ELEMENTOR],
  ['Back_Ma.TGA', 'Back_Ele.TGA'],
  'Back_Hero_EleCursemind.tga',
);
line(
  [JOB_PSYCHIKEEPER, JOB_PSYCHIKEEPER_MASTER, JOB_PSYCHIKEEPER_HERO],
  [JOB_VAGRANT, JOB_MAGICIAN, JOB_PSYCHIKEEPER],
  ['Back_Ma.TGA', 'Back_Psy.TGA'],
  'Back_Hero_PsyStone.tga',
);
line(
  [JOB_JESTER, JOB_JESTER_MASTER, JOB_JESTER_HERO],
  [JOB_VAGRANT, JOB_ACROBAT, JOB_JESTER],
  ['Back_Acr.tga', 'Back_Jst.TGA'],
  'Back_Hero_JstSilence.tga',
);
line(
  [JOB_RANGER, JOB_RANGER_MASTER, JOB_RANGER_HERO],
  [JOB_VAGRANT, JOB_ACROBAT, JOB_RANGER],
  ['Back_Acr.tga', 'Back_Rag.TGA'],
  'Back_Hero_RagHawkeye.tga',
);

/** The panel of a job; `undefined` for a job the window cannot show (the Puppeteer, the Gatekeeper and the Doppler have no tree). */
export function jobPanel(job: number): JobPanel | undefined {
  return PANELS.get(job);
}

/** `JOB_*` id of a hero's line is a hero when it is the `heroJob` of its line: the hero skill picture is shown. */
export function heroPicture(job: number): string | null {
  return PANELS.get(job)?.hero ?? null;
}
