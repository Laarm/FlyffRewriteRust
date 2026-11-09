// C `atoi` as the arithmetic window uses it (`CWndRRMiniGameArithmetic::OnChildNotify`, `src:_Interface/WndRainbowRace.cpp:1594`): leading white space, an optional sign, then decimal digits up to the first
// other character; a string without digits is 0. FIX: the result is clamped to the i32 range (the wire field is an `sint32`; the original's `atoi` overflowed at 2147483647).

export const I32_MAX = 2_147_483_647;
export const I32_MIN = -2_147_483_648;

/** `atoi( text )` clamped to i32. */
export function atoi(text: string): number {
  let i = 0;
  while (i < text.length && /\s/.test(text.charAt(i))) i += 1;
  let sign = 1;
  const c = text.charAt(i);
  if (c === '-' || c === '+') {
    if (c === '-') sign = -1;
    i += 1;
  }
  let n = 0;
  let digits = 0;
  while (i < text.length) {
    const code = text.charCodeAt(i);
    if (code < 48 || code > 57) break;
    n = n * 10 + (code - 48);
    digits += 1;
    // far beyond i32: stop accumulating (keeps the number exact enough to clamp)
    if (n > 1e12) {
      n = 1e12;
      break;
    }
    i += 1;
  }
  if (digits === 0) return 0;
  const v = sign * n;
  return v > I32_MAX ? I32_MAX : v < I32_MIN ? I32_MIN : v;
}
