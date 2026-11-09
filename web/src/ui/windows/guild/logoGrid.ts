// The geometry of the logo picker (`CWndGuildSetLogo`, `src:_Interface/WndGuildTabInfo.cpp:200-340`; WP-UI-46): logos 1..20 in a grid of 5 columns and 4 rows at
// `(24 + x + x * (w + 5), 18 + y + y * (h + 5))` of the window client (24 x 24 pictures), the selection is framed by two magenta outlines, one and two pixels outside the
// picture. A game master also gets the logos 21..27 (the original had a language-dependent GM button instead; the server accepts 21..27 for a game master): one row of
// seven below the grid. Pure numbers, goldens of the tests.

import type { Rect } from '../../layout/geometry';
import { GUILD_LOGO_COUNT, GUILD_LOGO_GM_FROM, guildLogoSize } from './logoPath';

export const LOGO_COLUMNS = 5;
/** Logos 1..20 are the normal ones (`CUSTOM_LOGO_MAX - 7`). */
export const LOGO_NORMAL_COUNT = GUILD_LOGO_GM_FROM - 1;
/** The y of the row of the game-master logos and the pitch of its cells. */
export const GM_ROW_Y = 178;
export const GM_ROW_X = 12;
export const GM_ROW_PITCH = 25;

/** The picture rectangle of logo `n` (1..27) in client coordinates, `null` for any other number. */
export function logoCell(n: number): Rect | null {
  if (!Number.isInteger(n) || n < 1 || n > GUILD_LOGO_COUNT) return null;
  const size = guildLogoSize(n);
  if (n < GUILD_LOGO_GM_FROM) {
    const i = n - 1;
    const x = i % LOGO_COLUMNS;
    const y = Math.floor(i / LOGO_COLUMNS);
    const l = 24 + x + x * (24 + 5);
    const t = 18 + y + y * (24 + 5);
    return { l, t, r: l + size, b: t + size };
  }
  const l = GM_ROW_X + (n - GUILD_LOGO_GM_FROM) * GM_ROW_PITCH;
  return { l, t: GM_ROW_Y, r: l + size, b: GM_ROW_Y + size };
}

/** The logo under a client point (`PtInRect( m_rect[i] )`), `0` for none; the GM logos only for a game master. */
export function logoAt(x: number, y: number, gm: boolean): number {
  const last = gm ? GUILD_LOGO_COUNT : LOGO_NORMAL_COUNT;
  for (let n = 1; n <= last; n += 1) {
    const r = logoCell(n);
    if (r && x >= r.l && x < r.r && y >= r.t && y < r.b) return n;
  }
  return 0;
}
