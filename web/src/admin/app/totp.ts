// The pure helpers of the two-factor dialog (Plan/25 section 1.3, WP-ADMIN-28): how the setup key is shown, how a code that was typed
// or pasted is read, and the data URL of the QR code. The dialog itself is `Totp.tsx`.

/** The setup key in groups of four (`GEZD GNBV GY3T ...`), the way authenticator apps print it. */
export function groupKey(key: string): string {
  return (key.replace(/\s+/g, '').match(/.{1,4}/g) ?? []).join(' ');
}

/** A code as the person typed or pasted it: spaces (also the non-breaking one) and hyphens go, so "123 456" and "123-456" read as 123456. */
export function normalizeCode(text: string): string {
  return text.replace(/[\s\u00a0-]/g, '');
}

/** Six digits, the only shape the server accepts. */
export function isCompleteCode(text: string): boolean {
  return /^\d{6}$/.test(normalizeCode(text));
}

/** An SVG document as a `data:` URL for an `<img>` (the admin CSP allows `img-src data:`; the markup is never inserted into the page). */
export function svgDataUrl(svg: string): string {
  let binary = '';
  for (const byte of new TextEncoder().encode(svg)) binary += String.fromCharCode(byte);
  return `data:image/svg+xml;base64,${btoa(binary)}`;
}
