// Job names and the three job generations (`JOB_*` of `gen/defs/job`): the console shows a job as text with its icon; the ids are the
// game's, the names are the English ones of the original (the texts bundle holds the translated ones, which a console does not need).
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
} from '../../../gen/defs/job';

const NAMES: ReadonlyMap<number, string> = new Map([
  [JOB_VAGRANT, 'Vagrant'],
  [JOB_MERCENARY, 'Mercenary'],
  [JOB_ACROBAT, 'Acrobat'],
  [JOB_ASSIST, 'Assist'],
  [JOB_MAGICIAN, 'Magician'],
  [JOB_PUPPETEER, 'Puppeteer'],
  [JOB_KNIGHT, 'Knight'],
  [JOB_BLADE, 'Blade'],
  [JOB_JESTER, 'Jester'],
  [JOB_RANGER, 'Ranger'],
  [JOB_RINGMASTER, 'Ringmaster'],
  [JOB_BILLPOSTER, 'Billposter'],
  [JOB_PSYCHIKEEPER, 'Psykeeper'],
  [JOB_ELEMENTOR, 'Elementor'],
  [JOB_GATEKEEPER, 'Gatekeeper'],
  [JOB_DOPPLER, 'Doppler'],
  [JOB_KNIGHT_MASTER, 'Knight'],
  [JOB_BLADE_MASTER, 'Blade'],
  [JOB_JESTER_MASTER, 'Jester'],
  [JOB_RANGER_MASTER, 'Ranger'],
  [JOB_RINGMASTER_MASTER, 'Ringmaster'],
  [JOB_BILLPOSTER_MASTER, 'Billposter'],
  [JOB_PSYCHIKEEPER_MASTER, 'Psykeeper'],
  [JOB_ELEMENTOR_MASTER, 'Elementor'],
  [JOB_KNIGHT_HERO, 'Knight'],
  [JOB_BLADE_HERO, 'Blade'],
  [JOB_JESTER_HERO, 'Jester'],
  [JOB_RANGER_HERO, 'Ranger'],
  [JOB_RINGMASTER_HERO, 'Ringmaster'],
  [JOB_BILLPOSTER_HERO, 'Billposter'],
  [JOB_PSYCHIKEEPER_HERO, 'Psykeeper'],
  [JOB_ELEMENTOR_HERO, 'Elementor'],
]);

/** The generation suffix of a job: master (ids 16..23) or hero (24..31). */
export function jobTier(job: number): '' | 'Master' | 'Hero' {
  if (job >= JOB_KNIGHT_HERO && job <= JOB_ELEMENTOR_HERO) return 'Hero';
  if (job >= JOB_KNIGHT_MASTER && job <= JOB_ELEMENTOR_MASTER) return 'Master';
  return '';
}

/** `Knight`, `Blade (Master)`, `Ranger (Hero)`; `Job 99` for an unknown id. */
export function jobName(job: number): string {
  const name = NAMES.get(job);
  if (name === undefined) return `Job ${String(job)}`;
  const tier = jobTier(job);
  return tier === '' ? name : `${name} (${tier})`;
}
