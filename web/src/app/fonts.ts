// Fonts at boot (Plan/40 section 3.1 B1c, PL-126, WP-CLIENT-11): `styles/fonts.css` declares the bundled DejaVu Sans
// subset as the family `FALLBACK_FONT_FAMILY`; the boot waits for exactly that family (`document.fonts.load`) with a
// 3 s timeout and continues without it when it does not come (the UI then renders in the fallback chain).

/** 42's `ui/text/fonts.ts` exports the same constant (42 section 8.1); `styles/fonts.css` uses the same name. */
export const FALLBACK_FONT_FAMILY = 'Flyff DejaVu Sans';
export const FONT_LOAD_TIMEOUT_MS = 3000;
/** The font request of boot step B1c: 12 px, the size of the UI text. */
export const FALLBACK_FONT_REQUEST = `12px "${FALLBACK_FONT_FAMILY}"`;

/** The slice of `document.fonts` the boot uses. */
export interface FontsLike {
  load(font: string): Promise<unknown>;
}

export type FontsResult = 'loaded' | 'timeout' | 'failed' | 'unavailable';

export interface FontTimers {
  setTimeout(cb: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
}

const GLOBAL_TIMERS: FontTimers = {
  setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
  clearTimeout: (id) => {
    globalThis.clearTimeout(id as number);
  },
};

/**
 * Requests the fallback font and waits at most `timeoutMs`. Never rejects: `timeout` and `failed` are logged by the
 * caller and the boot goes on (the UI falls back to the system fonts).
 */
export async function loadBootFonts(
  fonts: FontsLike | undefined,
  timers: FontTimers = GLOBAL_TIMERS,
  timeoutMs: number = FONT_LOAD_TIMEOUT_MS,
): Promise<FontsResult> {
  if (!fonts) return 'unavailable';
  let timer: unknown = null;
  const timeout = new Promise<'timeout'>((resolve) => {
    timer = timers.setTimeout(() => {
      resolve('timeout');
    }, timeoutMs);
  });
  const load = fonts
    .load(FALLBACK_FONT_REQUEST)
    .then((): FontsResult => 'loaded')
    .catch((): FontsResult => 'failed');
  try {
    return await Promise.race([load, timeout]);
  } finally {
    if (timer !== null) timers.clearTimeout(timer);
  }
}
