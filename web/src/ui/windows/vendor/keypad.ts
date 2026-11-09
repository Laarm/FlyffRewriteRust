// The number pad of the vendor windows (`CWndRegVend::OnChar` / `ProcessCalc`, `src:_Interface/WndRegVend.cpp:262-501`, and
// `CWndVendorBuy::OnChar`, `WndVendorBuy.cpp:262-313`; WP-UI-26): a number is a decimal STRING in a static (`GetTitle` / `SetTitle`),
// edited by key presses and by the pad buttons; the sell window also has a four-function calculator (`+ - * /` and `=`).
//
//   * `isFirst` is the state flag `m_bIsFirst`: set again by the first digit after anything that "closed" the number (an operator, `=`,
//     Esc); a digit with `isFirst == false` starts a NEW number (`str = "0"` first).
//   * the C `int` arithmetic is kept: a product that wraps negative becomes 2 100 000 000 on `=`; atoi saturates at INT_MAX (MSVC).
//   * FIX: a division by zero crashed the original (`SIGFPE`); here it leaves the number as it was.
//
// Pure functions on a `PadState`; the windows keep the state and show `text`.

/** `INT_MAX` (MSVC `atoi` saturates). */
export const INT_MAX = 0x7fff_ffff;
export const INT_MIN = -0x8000_0000;
/** The value the `=` key gives for a result that wrapped negative (`_itoa( 2100000000 )`). */
export const EQUALS_CAP = 2_100_000_000;

/** C `atoi`: optional blanks and sign, then digits; saturates at the `int` limits; 0 for anything else. */
export function atoi(text: string): number {
  const m = /^\s*([+-]?)(\d+)/.exec(text);
  if (!m) return 0;
  const n = Number(m[2]);
  if (m[1] === '-') return n > 2 ** 31 ? INT_MIN : -n;
  return n > INT_MAX ? INT_MAX : n;
}

/** The pending operator and its left operand (`CALC_DATA`). */
export interface Calc {
  value: number;
  ch: '' | '+' | '-' | '*' | '/';
}

/** One editable number plus the calculator state. */
export interface PadState {
  /** The text of the number (`GetTitle()`). */
  text: string;
  /** `m_bIsFirst`. */
  isFirst: boolean;
  calc: Calc;
}

/** The state of a pad whose number shows `text` (`OnInitialUpdate` + `SetItem`). */
export function initialPad(text: string): PadState {
  return { text, isFirst: true, calc: { value: 0, ch: '' } };
}

/** `ProcessCalc`: `calc.value (op) num` in C `int` arithmetic; 0 without an operator. */
export function processCalc(calc: Calc, num: number): number {
  switch (calc.ch) {
    case '*':
      return Math.imul(calc.value, num);
    case '/':
      return num === 0 ? calc.value : Math.trunc(calc.value / num) | 0;
    case '+':
      return (calc.value + num) | 0;
    case '-':
      return (calc.value - num) | 0;
    case '':
      return 0;
  }
}

/** The longest text a digit key may still extend (`strlen( str ) <= 9` before the append: ten digits at most); `null` = no limit. */
export interface PadLimits {
  readonly maxLength: number | null;
}

/** The sell window's limit: 10 characters. The buy window has none (its number is clamped to the stock). */
export const SELL_LIMITS: PadLimits = { maxLength: 9 };
export const BUY_LIMITS: PadLimits = { maxLength: null };

const OPERATORS = new Set(['+', '-', '*', '/']);

/**
 * `OnChar( nChar )`: one typed character on the number. `char` is one of `0`-`9`, `+ - * /` (the calculator keys; ignored when `calculator`
 * is false), `'\b'` (backspace) and `'\x1b'` (Escape: the number becomes `"0"`).
 */
export function padChar(
  s: PadState,
  char: string,
  limits: PadLimits,
  calculator: boolean,
): PadState {
  let { text, isFirst } = s;
  const calc = s.calc;
  if (char === '\x1b') {
    return { text: '0', isFirst: false, calc: { ...calc, value: 0 } };
  }
  if (char === '\b') {
    text = text.length > 1 ? text.slice(0, -1) : '0';
    return { text, isFirst, calc };
  }
  if (OPERATORS.has(char)) {
    if (!calculator) return s;
    if (isFirst && calc.value !== 0) {
      // a pending operation is finished first (`2 * 3 +` shows 6)
      text = String(processCalc(calc, atoi(text)));
    }
    return {
      text,
      isFirst: false,
      calc: { value: atoi(text), ch: char as Calc['ch'] },
    };
  }
  if (char >= '0' && char <= '9' && char.length === 1) {
    if (!isFirst) {
      text = '0';
      isFirst = true;
    }
    if (text !== '0') {
      if (limits.maxLength === null || text.length <= limits.maxLength) text += char;
    } else {
      text = char;
    }
    return { text, isFirst, calc };
  }
  return s;
}

/** `WIDC_NUM00` / `WIDC_NUM000`: two or three zeros appended unless that would pass the limit; nothing on `"0"`. */
export function padZeros(s: PadState, zeros: 2 | 3, limits: PadLimits): PadState {
  if (s.text === '0') return s;
  const next = s.text + '0'.repeat(zeros);
  if (limits.maxLength !== null && next.length > limits.maxLength) return s;
  return { ...s, text: next };
}

/** `WIDC_VENEQUA`: the result of the pending operation (only right after typed digits). */
export function padEquals(s: PadState): PadState {
  if (!s.isFirst) return s;
  const r = processCalc(s.calc, atoi(s.text));
  const text = r < 0 ? String(EQUALS_CAP) : String(r);
  return { text, isFirst: false, calc: { ...s.calc, value: 0 } };
}

/** `WIDC_VENMIN`: the number becomes `"1"`. */
export function padMin(s: PadState): PadState {
  return { ...s, text: '1' };
}

/** The character of a key press (`KeyboardEvent.code`, English layout), `null` for a key the pad ignores. */
export function charOfKey(code: string, shift: boolean): string | null {
  const digit = /^(?:Digit|Numpad)(\d)$/.exec(code);
  if (digit) {
    // Shift+8 is the `*` of the US layout
    if (code === 'Digit8' && shift) return '*';
    if (shift && code.startsWith('Digit')) return null;
    return digit[1] ?? null;
  }
  switch (code) {
    case 'Backspace':
      return '\b';
    case 'Escape':
      return '\x1b';
    case 'NumpadAdd':
      return '+';
    case 'Equal':
      return shift ? '+' : null;
    case 'NumpadSubtract':
    case 'Minus':
      return shift && code === 'Minus' ? null : '-';
    case 'NumpadMultiply':
      return '*';
    case 'NumpadDivide':
    case 'Slash':
      return '/';
    default:
      return null;
  }
}
