// Copy to the clipboard without `navigator.clipboard` (it needs a secure context and the console also runs on plain http, which is
// the supported deployment: eslint/rules.js bans it). A hidden textarea and `execCommand('copy')` work on every origin.

/** Copies `text`; `true` when the browser accepted it. */
export function copyText(text: string): boolean {
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  area.style.position = 'fixed';
  area.style.top = '-1000px';
  document.body.appendChild(area);
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  area.select();
  let ok: boolean;
  try {
    // deprecated but universal: the one way that works without a secure context (navigator.clipboard is banned, see above)
    // eslint-disable-next-line @typescript-eslint/no-deprecated
    ok = document.execCommand('copy');
  } catch {
    ok = false;
  }
  area.remove();
  previous?.focus();
  return ok;
}

/** The full URL of a route hash (`#/players/characters?c=12`) for "Copy link". */
export function linkTo(hash: string): string {
  return `${location.origin}${location.pathname.replace(/\/$/, '')}${hash}`;
}
