// The number helpers of the buy and sell confirmations (`WndConfirmBuy`, `WndConfirmSell`), shared by the two lazy shells.

/** `INT_MAX`: a total above it takes one piece back (`OnChangeBuyCount`). */
export const INT_MAX = 0x7fff_ffff;

/** `atoi` of the edit text: leading digits, 0 for anything else. */
export function atoi(text: string): number {
  const m = /^\s*[+-]?\d+/.exec(text);
  if (!m) return 0;
  const n = Number.parseInt(m[0], 10);
  return Number.isFinite(n) ? n : 0;
}
