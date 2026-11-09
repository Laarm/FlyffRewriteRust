// `atoi` for the penya edit of the contribution window (`m_nPenya = atoi( pWndEdit->GetString() )`; a negative number becomes 0).

/** The leading digits of the text as a number, 0 for none (and for anything that is not a finite number). */
export function atoiPenya(text: string): number {
  const m = /^\s*(\d+)/.exec(text);
  const n = m?.[1] === undefined ? 0 : Number(m[1]);
  return Number.isFinite(n) ? n : 0;
}
