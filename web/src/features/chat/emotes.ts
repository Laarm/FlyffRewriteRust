// Emoticons (Plan/45 §8.5; `CDialogMsg::AddMessage`, DialogMsg.cpp:813-860): a `ChatNtf` text that is exactly `"/" + command`
// of `texts.emotions` (CASE-SENSITIVE, KEEP-QUIRK) is the chat emoticon of that row, a text `!N` is dialog emoticon N, any
// other text starting with `/` or `!` is a plain balloon without a chat line.

export interface EmotionRow {
  readonly id: number;
  readonly text: string;
}

/** `MAX_EMOTICON_NUM` of the dialog emoticons (`!N` is clamped to `0..100`, 45 §8.2). */
export const DIALOG_EMOTICON_MAX = 100;

export type EmoteMatch =
  | { readonly kind: 'chat'; readonly index: number }
  | { readonly kind: 'dialog'; readonly index: number }
  | { readonly kind: 'balloon' };

/** `"/laugh"` -> chat emoticon of the row `laugh`; `"!3"` -> dialog emoticon 3; other `/` / `!` text -> a plain balloon. */
export function matchEmote(text: string, emotions: readonly EmotionRow[]): EmoteMatch | null {
  if (text.startsWith('/')) {
    for (const e of emotions) {
      if (text === `/${e.text}`) return { kind: 'chat', index: e.id };
    }
    return { kind: 'balloon' };
  }
  if (text.startsWith('!')) {
    const m = /^!\s*(-?\d+)/.exec(text);
    // `scanner.GetNumber()` of a non-number reads 0
    const n = m?.[1] === undefined ? 0 : Number.parseInt(m[1], 10);
    return { kind: 'dialog', index: Math.max(0, Math.min(DIALOG_EMOTICON_MAX, n)) };
  }
  return null;
}
