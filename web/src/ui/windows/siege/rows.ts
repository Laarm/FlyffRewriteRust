// The pure helpers of the siege windows (WP-UI-49..51): the bid parser, the rows of the lists and the numbers the original draws with. They live apart from the window classes because the
// service and the tests read them and the classes themselves are lazy chunks (a lazy class may be imported by `lazy.ts` only).

import { WIDC_BUTTON1, WIDC_BUTTON2, WIDC_BUTTON3, WIDC_BUTTON4 } from '../../../gen/defs/resdata';
import { cutName, head, pad2, padLeft } from './format';
import { I32_MAX, type SelectionMember } from './ports';
import type { TableRow } from './TableList';

const BLACK = 0xff000000;
const RED = 0xffc80000;

/** The digits of the edit: at most ten. */
export const OFFER_MAX_DIGITS = 10;

/** `atoi( str )` of a digit string, clamped to `INT_MAX` (an empty string is 0). */
export function parseBid(text: string): number {
  const digits = text.replace(/[^0-9]/g, '');
  if (digits === '') return 0;
  const n = Number(digits);
  return Math.min(n, I32_MAX);
}

/** The colour of the own rank outside the admitted guilds (`m_dwColor = 0xFFFF0000`). */
export const RANK_OUT_COLOR = 0xffff0000;

/** One list row: `"No.%.2d\t\t<guild>"`. */
export function guildRow(index: number, guild: string): string {
  return `No.${pad2(index)}\t\t${guild}`;
}

/** The row pitch the original's bands and the "D" marker use (`i * 17 + 75`). */
export const SELECTION_PITCH = 17;
/** The band geometry of `OnDraw`: x 349..587 from y 74; the waiting players' band ends at y 377. */
export const BAND = { l: 349, r: 587, t: 74, b: 377 } as const;
/** The "D" button sits at `(330, index * 17 + 75)`. */
export const DEFENDER_MARK = { x: 330, y0: 75 } as const;

/** `"Lv%.2d\t%.16s %.10s"`. */
export function memberRow(m: SelectionMember): string {
  return `Lv${pad2(m.level)}\t${head(m.name, 16)} ${head(m.job, 10)}`;
}

/** The gate buttons in gate order. */
export const GATE_BUTTONS = [WIDC_BUTTON1, WIDC_BUTTON2, WIDC_BUTTON3, WIDC_BUTTON4] as const;

/** The colour of the frame round the chosen gate. */
export const GATE_FRAME_COLOR = 'rgb(0, 0, 255)';

/** The rows of `OnDraw`: `rbegin()` of the multimap, dense ranks, the rank blank on a continuation row. */
export function rankingRows(guilds: readonly { name: string; wins: number }[]): TableRow[] {
  // a multimap keeps equal keys in insertion order; read from the end, the later guild comes first
  const sorted = guilds
    .map((g, i) => ({ g, i }))
    .sort((a, b) => a.g.wins - b.g.wins || a.i - b.i)
    .reverse();
  let rank = 0;
  let old = -1;
  return sorted.map(({ g }, row) => {
    const first = old !== g.wins;
    if (first) rank += 1;
    old = g.wins;
    const color = row === 0 ? RED : BLACK;
    return {
      color,
      cells: [
        first ? { x: 12, text: padLeft(rank, 3) } : { x: 13, text: '   ' },
        { x: 48, text: cutName(g.name, 8) },
        { x: 188, text: padLeft(g.wins, 2) },
      ],
    };
  });
}

/** `MAX_GUILDCOMBAT_LIST`: the original refuses more entries than this. */
export const WAR_STATE_MAX = 100;

export function warStateRows(
  entries: readonly { guild: string; master: string; count: number }[],
): TableRow[] {
  return entries
    .filter((e) => e.count !== 0)
    .slice(0, WAR_STATE_MAX)
    .map((e, i) => ({
      color: BLACK,
      cells: [
        { x: 12, text: padLeft(i + 1, 3) },
        { x: 38, text: cutName(e.guild, 8) },
        { x: 168, text: e.master },
        { x: 268, text: padLeft(e.count, 2) },
      ],
    }));
}
