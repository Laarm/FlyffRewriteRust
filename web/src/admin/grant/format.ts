// Number formatting of the grant UI: en-US thousands separators (the console is English, D1), so a test and an operator read the same text.
const NUMBER = new Intl.NumberFormat('en-US');

/** `1234567` as `1,234,567`. */
export function formatNumber(n: number): string {
  return NUMBER.format(n);
}
