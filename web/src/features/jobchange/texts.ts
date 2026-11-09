// The class texts of the fast job change window (`CWndJobChangeEx::OnDraw`, `src:_Interface/WndChangeJobEx.cpp:80-98`; Plan/45 §21):
// the English strings of the original, copied verbatim (they are not in the texts bundle; the typos and the `<Leer>` placeholders are
// the original's). Index 0 is the error entry, 1-4 the first jobs, 6-13 the second jobs, 16 the master text, 17 the hero text.

import { MAX_HERO, MAX_MASTER, MAX_PROFESSIONAL } from '../../gen/defs/job';

export const CLASS_TEXTS: readonly string[] = [
  'Error',
  'Mercenaries prefer to partake in close range combat. The main characteristics of this particular class involve physical strength, and a strong body that is able to withstand attacks.',
  'Acrobats are those who mainly wield long distance weapons such as bows or yoyos. Using the skill tactics of speed and accuracy, they are usually able to break anyone willing enough to fight them.',
  "Being an assist, you'll be able to choose to either be a fighter, or a healer. If a fighter is what you desire, assists generally favouritise close and personal combat. If supporting other characters is more your style, you may choose the alternative.",
  'Magicians are ranged spell casters, they are able to focus large amounts of magic upon an enemy, dealing a great deal of damage before they are even reached. Magicians generally have lower defense, lower HP, and sometimes even delay when it comes to casting spells.',
  '<Leer>',
  'Knights are a second class for mercenaries and are the best tanks in the game. As a knight you will receive more health points per stamina point than any other class.',
  'Blades are a second class for mercenaries and are the best damage dealers in the game. As a blade you will be able to wield two weapons at the same time and thus drastically increase your damage output.',
  "Jesters focus on ranged attacks using Yo-Yos. They have a wide range of skills that allow them to be strong PvP'ers with their YoYo attacks, use some of their penya to do increased damage, and escape incoming attacks.",
  'Rangers are a ranged based melee class that focus on using bow skills to damage enemies.',
  'When it comes to partners, this class is the most sought after due to their ability to enhance others abilities and skills. With extra buffs and skills, this assist class is almost considered a necessity for game play; or, at least, they make it easier.',
  'Billposters are undoubtedly the most self-sufficient class in the game. With their normal assist buffs, and their additional damage skills upon second job change, they are a very strong class. They are more of a melee class, and fight using a knuckle and a shield.',
  "Psykeepers are one of the Magician's second classes. These, however, do not deal in the field of elemental damage. Most of the attacks, which are very powerful, tend to be based off mental and demonic forces.",
  'Elementors are a second class for magicians and are a high damage spell casting class that uses staves to cast devastating elemental spells. ',
  '<Leer>',
  '<Leer>',
  'Become master to get more strength',
  'Become hero to get more strength',
];

/** The text of a class: the entry of the job itself below the professions, the master text for a master, the hero text for a hero. */
export function classText(job: number): string {
  if (job < MAX_PROFESSIONAL) return CLASS_TEXTS[job] ?? '';
  if (job < MAX_MASTER) return CLASS_TEXTS[MAX_PROFESSIONAL] ?? '';
  if (job < MAX_HERO) return CLASS_TEXTS[MAX_PROFESSIONAL + 1] ?? '';
  return CLASS_TEXTS[job - 14] ?? '';
}
