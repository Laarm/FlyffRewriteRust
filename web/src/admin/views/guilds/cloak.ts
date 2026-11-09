// The cloak of a guild logo (Plan/25 section 6.2 step 3, `src:WORLDSERVER/DPSrvr.cpp:1291-1387` `OnCreateGuildCloak`): which texture the preview
// shows and which item the server gives. Pure and tested; the server's `cloak_item_id(logo, as_gm = true)` is the golden of the item ids.
//
//   logo 0          no logo, no cloak
//   logo 1..20      `SYSCLOAK01 + logo - 1`, texture `Part_{male,female}CloakSLogo<logo>.dds`
//   logo 21..27     the game-master logos give the SYSCLOAK01 cloak (the original: GM and logo > CUSTOM_LOGO_MAX - 7), texture `...01`
//   logo 999        the blank cloak (`II_ARM_S_CLO_CLO_BLANK`), texture `...00`
import { II_ARM_S_CLO_CLO_BLANK, II_ARM_S_CLO_CLO_SYSCLOAK01 } from '../../../gen/defs/item';
import { GUILD_LOGO_COUNT, GUILD_LOGO_GM_FROM } from '../../../features/guild/logos';

/** The logo that asks for the blank (custom) cloak. */
export const LOGO_CUSTOM = 999;

export type CloakSex = 'male' | 'female';

/** The number of the texture file a logo uses (`00` for the blank cloak, `null` for no logo). */
export function cloakTextureNumber(logo: number): number | null {
  if (logo === LOGO_CUSTOM) return 0;
  if (!Number.isInteger(logo) || logo < 1 || logo > GUILD_LOGO_COUNT) return null;
  return logo >= GUILD_LOGO_GM_FROM ? 1 : logo;
}

/** The lower-cased VFS path of the cloak texture (`model/texture/part_malecloakslogo05.dds`), `null` for no logo. */
export function cloakTexturePath(logo: number, sex: CloakSex): string | null {
  const n = cloakTextureNumber(logo);
  return n === null ? null : `model/texture/part_${sex}cloakslogo${String(n).padStart(2, '0')}.dds`;
}

/** The item id of the cloak the server gives for a logo (`null` for no logo). */
export function cloakItemId(logo: number): number | null {
  if (logo === LOGO_CUSTOM) return II_ARM_S_CLO_CLO_BLANK;
  if (!Number.isInteger(logo) || logo < 1 || logo > GUILD_LOGO_COUNT) return null;
  return logo >= GUILD_LOGO_GM_FROM
    ? II_ARM_S_CLO_CLO_SYSCLOAK01
    : II_ARM_S_CLO_CLO_SYSCLOAK01 + logo - 1;
}
