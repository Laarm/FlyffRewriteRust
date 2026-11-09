// Client formatting of `DefinedText` (Plan/11 section 10.1, WP-PROTO-20). The server sends the TID and the
// already formatted arguments as a list of strings; the client resolves the printf-style template of the TID
// from the text bundle and substitutes positionally.

/** One printf conversion (`%s`, `%d`, `%5.2f`, `%ld`, `%I64d`, ...) or an escaped percent sign (`%%`). */
const CONVERSION = /%(?:%|[-+ #0]*\d*(?:\.\d+)?(?:hh|h|ll|l|L|z|j|t|I64)?[sduixXcfgGeEopU])/g;

/**
 * Replaces the i-th printf conversion of `template` with `args[i]`. A missing argument becomes the empty
 * string, extra arguments are ignored, and `%%` is a literal percent sign that consumes no argument.
 * All arguments are strings: the server already formatted every number as the original `%d` / `%s` output
 * (money with the original thousands separators), so the conversion letter never changes the text.
 */
export function formatDefinedText(template: string, args: readonly string[]): string {
  let next = 0;
  return template.replace(CONVERSION, (conversion) => {
    if (conversion === '%%') {
      return '%';
    }
    const arg = args[next] ?? '';
    next += 1;
    return arg;
  });
}
