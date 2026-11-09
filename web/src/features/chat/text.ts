// Small text helpers of the chat feature.

/** The first `max` Unicode scalar values (`SetStrNull`). */
export function cutChars(s: string, max: number): string {
  const chars = Array.from(s);
  return chars.length <= max ? s : chars.slice(0, max).join('');
}
