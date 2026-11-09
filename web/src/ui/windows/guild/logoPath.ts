// The guild logos (`CWndWorld::m_pTextureLogo[CUSTOM_LOGO_MAX]`, `src:_Interface/WndWorld.cpp:3850-3860`): 27 pictures `Icon_CloakSLogo01..27.jpg`
// of `Resource/Icon`. 1..20 are the logos a master may pick, 21..27 belong to game masters (the original had one language-dependent GM button whose logo
// table is not in the source; the server accepts 21..27 for a game master). A pure module without React and without `ui/` imports, so that the
// renderer, the siege windows and the admin logo picker can use it (`features/guild/logos.ts` re-exports it).

/** `CUSTOM_LOGO_MAX`. */
export const GUILD_LOGO_COUNT = 27;
/** The first logo only a game master may set. */
export const GUILD_LOGO_GM_FROM = 21;
/** The size of a logo picture in pixels (the picker's grid pitch): 24 x 24, the logos 22 and 27 are 20 x 20 (the real files). */
export const GUILD_LOGO_SIZE = 24;

/** The size of the picture of logo `n` in pixels: 24 x 24, the logos 22 and 27 are 20 x 20 (`Icon_CloakSLogo22.jpg` and `27`, checked on the Resource). */
export function guildLogoSize(n: number): number {
  return n === 22 || n === GUILD_LOGO_COUNT ? 20 : GUILD_LOGO_SIZE;
}

/** The lower-cased VFS path of a logo picture (`icon/icon_cloakslogo03.jpg`), `null` for a number outside 1..27. */
export function guildLogoPath(n: number): string | null {
  if (!Number.isInteger(n) || n < 1 || n > GUILD_LOGO_COUNT) return null;
  return `icon/icon_cloakslogo${String(n).padStart(2, '0')}.jpg`;
}

/** Whether a logo can be chosen in the picker: 1..20 for everybody, 21..27 for a game master. */
export function isPickableLogo(n: number, isGm: boolean): boolean {
  if (!Number.isInteger(n) || n < 1 || n > GUILD_LOGO_COUNT) return false;
  return n < GUILD_LOGO_GM_FROM || isGm;
}
