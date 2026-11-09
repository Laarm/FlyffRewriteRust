// The pure values of the mini-game windows (not in a lazy window file: `index.ts` and the feature read them as values, and a value import from a lazy module is an `INEFFECTIVE_DYNAMIC_IMPORT`).

/** The control ids of the six faces of the dice window, in face order: the stake buttons, the three rectangle families (`STATIC2+i`, `STATIC8+i`, `STATIC14+i`) and the cost texts. */
export const DICE_IDS = {
  buttons: [889, 648, 545, 515, 516, 404],
  stake: [900, 936, 867, 872, 886, 862],
  payout: [863, 865, 866, 787, 529, 1107],
  small: [483, 481, 1110, 1111, 1112, 1131],
  cost: [690, 1058, 1205, 1206, 1207, 1208],
  rcost: [1209, 1210, 1211, 1212, 1213, 1214],
} as const;

/** The stake text is blue, the payout text purple. */
export const STAKE_COLOR = 0xff4d4dff;
export const PAYOUT_COLOR = 0xff9932cd;

/** C `_ttoi`: optional blanks, an optional sign, digits; stops at the first other character; saturates at the i32 range. */
export function ttoi(text: string): number {
  const m = /^[ \t\n\v\f\r]*([+-]?)(\d*)/.exec(text);
  if (!m) return 0;
  const digits = m[2] ?? '';
  if (digits === '') return 0;
  const n = Number.parseInt(digits, 10) * (m[1] === '-' ? -1 : 1);
  return Math.max(-2_147_483_648, Math.min(2_147_483_647, n));
}
