// A small C `printf` for the legacy text templates of the tooltips (`str.Format(prj.GetText(tid), args)`). The shared
// `formatTemplate` of `features/common/text.ts` substitutes the i-th conversion by `String(arg)` and ignores flags, width and
// precision; the tooltip templates need them (`"%.2d:%.2d"` is `05:03`, `"%+d"` prints the sign, `"%.1f"` rounds).
//
// Supported: `%%`, flags `- + space # 0`, width, `.precision`, length modifiers (`h l ll I64`) and the conversions `d i u s c f x X`.
// A missing argument prints nothing for `s`/`c` and `0` for the numbers; extra arguments are ignored (PL-117).

export type PrintfArg = string | number | bigint;

const SPEC = /%(?:%|([-+ #0]*)(\d*)(?:\.(\d*))?(?:I64|ll|l|h)?([sdiucfxX]))/g;

function pad(body: string, width: number, left: boolean, zero: boolean, signLen: number): string {
  if (body.length >= width) return body;
  const fill = width - body.length;
  if (left) return body + ' '.repeat(fill);
  if (zero) return body.slice(0, signLen) + '0'.repeat(fill) + body.slice(signLen);
  return ' '.repeat(fill) + body;
}

/** Rounds half away from zero at `digits` decimals the way C prints a (binary) double: via `toFixed` on the exact value. */
function fixed(n: number, digits: number): string {
  if (!Number.isFinite(n)) return n < 0 ? '-inf' : Number.isNaN(n) ? 'nan' : 'inf';
  return n.toFixed(digits);
}

/** `sprintf(template, ...args)`. */
export function sprintf(template: string, args: readonly PrintfArg[] = []): string {
  let next = 0;
  return template.replace(SPEC, (whole, flagsRaw, widthRaw, precRaw, conv) => {
    if (whole === '%%') return '%';
    const flags = (flagsRaw ?? '') as string;
    const left = flags.includes('-');
    const plus = flags.includes('+');
    const space = flags.includes(' ');
    const zero = flags.includes('0') && !left;
    const width = widthRaw ? Number(widthRaw) : 0;
    const hasPrec = precRaw !== undefined;
    const prec = hasPrec ? Number(precRaw === '' ? '0' : precRaw) : -1;
    const arg = args[next];
    next += 1;
    switch (conv) {
      case 's': {
        let s = arg === undefined ? '' : String(arg);
        if (hasPrec) s = s.slice(0, prec);
        return pad(s, width, left, false, 0);
      }
      case 'c': {
        const s =
          arg === undefined
            ? ''
            : typeof arg === 'number'
              ? String.fromCharCode(arg)
              : String(arg).slice(0, 1);
        return pad(s, width, left, false, 0);
      }
      case 'd':
      case 'i':
      case 'u': {
        let n = typeof arg === 'bigint' ? arg : BigInt(Math.trunc(Number(arg ?? 0)) || 0);
        if (conv === 'u' && n < 0n) n = BigInt.asUintN(32, n);
        const neg = n < 0n;
        let digits = (neg ? -n : n).toString();
        if (hasPrec && digits.length < prec) digits = '0'.repeat(prec - digits.length) + digits;
        if (hasPrec && prec === 0 && n === 0n) digits = '';
        const sign = neg ? '-' : plus && conv !== 'u' ? '+' : space && conv !== 'u' ? ' ' : '';
        return pad(sign + digits, width, left, zero && !hasPrec, sign.length);
      }
      case 'x':
      case 'X': {
        const raw = BigInt.asUintN(32, BigInt(Math.trunc(Number(arg ?? 0)) || 0));
        let digits = raw.toString(16);
        if (conv === 'X') digits = digits.toUpperCase();
        if (hasPrec && digits.length < prec) digits = '0'.repeat(prec - digits.length) + digits;
        return pad(digits, width, left, zero && !hasPrec, 0);
      }
      case 'f': {
        const v = Number(arg ?? 0);
        const body = fixed(Math.abs(v), hasPrec ? prec : 6);
        const sign = v < 0 || Object.is(v, -0) ? '-' : plus ? '+' : space ? ' ' : '';
        return pad(sign + body, width, left, zero, sign.length);
      }
      default:
        return whole;
    }
  });
}
