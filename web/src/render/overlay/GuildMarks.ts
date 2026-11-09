// Guild marks above a player (Plan/41 §19.3, WP-REND-26): the master / hero icon, the guild logo and the two siege marks (best
// player, winning guild). Pure layout over the screen anchor of the name: `MoverRender.cpp:1611-1855` (icons, logo, marks),
// `WndWorld.cpp:3852-3860` (the 27 logo pictures), `Neuz.cpp:1668-1674` (the master and hero icons).
//
// Every number is an original screen pixel; `ax = even(trunc(anchor.x))`, `ay = trunc(anchor.y)`, the extents are the widths of the
// name label and of the guild name, and integer division is C's (`trunc(ext / 2)`). The sizes of the pictures are the textures'
// (`OverlayAssets.image(path)`): an image that has not arrived draws nothing (and starts loading).
//
//   master / hero icon   x = ax + trunc(ext / 2),                      y = ay - (iconH + 5)          alpha 255
//   logo                 x = ax - trunc(ext / 2) - (logoW + 2),         y = ay - (logoH + 5)          alpha 255
//   best / winner mark   with a logo:    x = logo.x - (logoW + 2),      y = logo.y          (the LOGO's width, not the mark's)
//                        without a logo: x = ax - trunc(L / 2) - 30,    y = ay - 30
//                        where L is the extent of whichever of name / guild has the larger BYTE length (`strlen`, a tie takes the
//                        guild: KEEP-QUIRK, not the pixel maximum)
//
// FIX of the original: a logo number outside 1..27 (the legacy 999) read past `m_pTextureLogo` and draws nothing here (the marks then
// follow the no-logo rule); a master level outside 60..120 left `pTexture` uninitialised and draws no icon.

import type { ImageItem, LegendClass, OverlayAssets, OverlaySubject, ViewerSiege } from './types';

/** `CUSTOM_LOGO_MAX`: the number of logo pictures (`Icon_CloakSLogo01..27.jpg`). */
export const GUILD_LOGO_COUNT = 27;

/**
 * The VFS path of logo `n` (`icon/icon_cloakslogo07.jpg`), `null` outside 1..27. A copy of `features/guild/logos.ts` (render does not
 * import features; `features/guildmarks` has a test that compares the two).
 */
export function guildLogoPath(n: number): string | null {
  if (!Number.isInteger(n) || n < 1 || n > GUILD_LOGO_COUNT) return null;
  return `icon/icon_cloakslogo${String(n).padStart(2, '0')}.jpg`;
}

/** `g_Neuz.m_pMasterIcon[0..5]` (`icon_Expert1..6.dds`). */
export const MASTER_ICON_PATHS: readonly string[] = [1, 2, 3, 4, 5, 6].map(
  (n) => `icon/icon_expert${String(n)}.dds`,
);
/** `g_Neuz.m_pHeroIcon`. */
export const HERO_ICON_PATH = 'icon/icon_hero.dds';
/** `CWndWorld::m_TexGuildBest` / `m_TexGuildWinner` (`theme/TexGuildCombatBest.bmp`, `TexGuildCombatWinner.bmp`, 25 x 25). */
export const BEST_MARK_PATH = 'theme/texguildcombatbest.bmp';
export const WINNER_MARK_PATH = 'theme/texguildcombatwinner.bmp';

/** `CGuildCombat::CLOSE_STATE`: the siege is closed and its results are shown (the two marks). */
export const SIEGE_CLOSE_STATE = 1;
/** `CGuildCombat::MAINTENANCE_STATE`: from here on a defender is coloured. */
export const SIEGE_MAINTENANCE_STATE = 103;
/** `0xFFFEA8FF`: the name colour of a defender of the siege (the guild line). */
export const COLOR_SIEGE_DEFENDER = 0xfffea8ff;

/** The guild logo pictures (`icon/icon_cloakslogo01..27.jpg`) are the ones the original loaded with the colour key pure black. */
export function isLogoPicture(path: string): boolean {
  return /^icon\/icon_cloakslogo\d\d\.jpg$/.test(path);
}

/**
 * The pure black colour key of a logo picture (`D3DCOLOR_XRGB( 0, 0, 0 )` of `LoadTexture`): every pixel that is exactly `(0, 0, 0)` becomes transparent. The
 * pictures are JPEGs, so the noise around an edge leaves most near-black pixels opaque, as it did in the original. A copy; the input stays as it is.
 */
export function keyBlackRgba(data: Uint8ClampedArray): Uint8ClampedArray {
  const out = new Uint8ClampedArray(data);
  for (let i = 0; i + 3 < out.length; i += 4) {
    if (out[i] === 0 && out[i + 1] === 0 && out[i + 2] === 0) out[i + 3] = 0;
  }
  return out;
}

/** The logo number as a picture: 1..27, anything else (0, the legacy 999, a fraction) is no logo. */
export function validLogo(logo: number | undefined): number {
  return logo !== undefined && Number.isInteger(logo) && logo >= 1 && logo <= GUILD_LOGO_COUNT
    ? logo
    : 0;
}

/**
 * The master icon index (0..5) of a level: 60-69, 70-79, 80-89, 90-99, 100-109, 110-120. `null` outside 60..120 (the original left the
 * texture pointer uninitialised there).
 */
export function masterIconIndex(level: number): number | null {
  if (!(level >= 60 && level <= 120)) return null;
  return Math.min(5, Math.floor(level / 10) - 6);
}

/** The icon picture of a legend class at a level, `null` for none. */
export function legendIconPath(
  legend: LegendClass | null | undefined,
  level: number,
): string | null {
  if (legend === 'hero') return HERO_ICON_PATH;
  if (legend === 'master') {
    const i = masterIconIndex(level);
    return i === null ? null : (MASTER_ICON_PATHS[i] ?? null);
  }
  return null;
}

/** The siege defender rule (`g_GuildCombatMng.m_nGCState >= MAINTENANCE_STATE && GetGCStatusDefender(m_idPlayer) != -1`). */
export function isSiegeDefender(s: OverlaySubject, siege: ViewerSiege | null | undefined): boolean {
  return (
    siege !== null &&
    siege !== undefined &&
    siege.warState >= SIEGE_MAINTENANCE_STATE &&
    s.idPlayer !== undefined &&
    siege.defenders.has(s.idPlayer)
  );
}

/** `strlen`: the byte length of a name (UTF-8; the original names are single-byte). */
export function byteLength(text: string): number {
  let n = 0;
  for (const ch of text) {
    const c = ch.codePointAt(0) ?? 0;
    n += c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
  }
  return n;
}

/** The widths the layout needs: the pixel extents of the name label and the guild name and their byte lengths. */
export interface MarkExtents {
  /** `GetTextExtent(szName).cx`: the full label (title prefix and staff suffix). */
  readonly labelW: number;
  /** `GetTextExtent(m_szGuild).cx`, 0 without a guild or for an unnamed guild. */
  readonly guildW: number;
  readonly labelBytes: number;
  readonly guildBytes: number;
}

/** What {@link layoutGuildMarks} returns: the icon (drawn before the guild text) and the logo with its mark (after it). */
export interface GuildMarkItems {
  readonly icon: ImageItem | null;
  readonly logo: readonly ImageItem[];
}

const NONE: GuildMarkItems = { icon: null, logo: [] };

function image(path: string, x: number, y: number): ImageItem {
  return { kind: 'image', path, x, y, alpha: 255, scaleX: 1, scaleY: 1 };
}

/**
 * The icon, logo and marks of one player. `ax` / `ay` are the even-adjusted anchor in whole pixels. The caller has applied the name
 * rules (`nameplateShown`) and the text-disguise skip; this function decides the rest. Nothing is drawn for a picture that has not
 * arrived yet.
 */
export function layoutGuildMarks(
  s: OverlaySubject,
  ax: number,
  ay: number,
  e: MarkExtents,
  siege: ViewerSiege | null | undefined,
  assets: OverlayAssets,
): GuildMarkItems {
  if (s.kind !== 'player') return NONE;
  const ext = Math.max(e.labelW, e.guildW);
  let icon: ImageItem | null = null;
  const iconPath = legendIconPath(s.legend, s.level ?? 0);
  if (iconPath !== null) {
    const img = assets.image(iconPath);
    if (img) icon = image(iconPath, ax + Math.trunc(ext / 2), ay - (img.h + 5));
  }
  const g = s.guild;
  if (!g) return { icon, logo: [] };

  // the two siege marks: the best player first, else a member of the winning guild; never both
  let markPath: string | null = null;
  if (siege?.state === SIEGE_CLOSE_STATE) {
    if (siege.bestPlayerId !== 0 && siege.bestPlayerId === s.idPlayer) markPath = BEST_MARK_PATH;
    else if (siege.winGuildId !== 0 && siege.winGuildId === g.id) markPath = WINNER_MARK_PATH;
  }
  const logoNo = validLogo(g.logo);
  const logoPath = logoNo === 0 ? null : guildLogoPath(logoNo);
  const out: ImageItem[] = [];
  if (logoPath !== null) {
    const logo = assets.image(logoPath);
    if (!logo) return { icon, logo: [] };
    const x = ax - Math.trunc(ext / 2) - (logo.w + 2);
    const y = ay - (logo.h + 5);
    out.push(image(logoPath, x, y));
    if (markPath !== null && assets.image(markPath)) {
      out.push(image(markPath, x - (logo.w + 2), y));
    }
  } else if (markPath !== null && assets.image(markPath)) {
    // without a logo: whichever of name and guild has the larger byte length decides the extent, a tie takes the guild
    const w = e.labelBytes > e.guildBytes ? e.labelW : e.guildW;
    out.push(image(markPath, ax - Math.trunc(w / 2) - 30, ay - 30));
  }
  return { icon, logo: out };
}
