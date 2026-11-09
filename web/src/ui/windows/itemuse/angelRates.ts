// The numbers of the angel summoning window that others read (the gallery, the tests): kept out of the lazy shell (`ui/wnd/lazy.ts`).

/** `MAX_MATDIE`. */
export const MAX_MATDIE = 20;

/** The four rates of the materials (`SummonRateRefresh`). */
export function angelRates(
  orichalcum: number,
  moonstone: number,
): {
  readonly red: number;
  readonly blue: number;
  readonly green: number;
  readonly white: number;
} {
  const green = orichalcum * 1 + moonstone * 1;
  const white = green / 10;
  const blue = green * 2;
  const red = 100 - (green + blue);
  return { red, blue, green, white };
}
