// A guild logo (Plan/25 section 2.4, 6.1): the original 24 x 24 picture (`Icon_CloakSLogo01..27.jpg`; the logos 22 and 27 are 20 x 20) at an
// integer scale with pixelated rendering, an empty frame for "no logo" and a chip for the custom logo 999. It reaches the game images
// through `views/players/gamedata`, so the shell never imports it (`isolation.test.ts`): the views of the guild category and the grant
// dialog's guild picker do.
import {
  GUILD_LOGO_COUNT,
  GUILD_LOGO_SIZE,
  guildLogoPath,
  guildLogoSize,
} from '../../features/guild/logos';
import { useAssetUrl } from '../views/players/gamedata';

export interface GuildLogoProps {
  /** 0 = none, 1..27, 999 = a custom cloak. */
  readonly logo: number;
  /** Integer scale of the 24 px picture (the list shows 2, the drawer 3). */
  readonly scale?: number;
  /** The accessible name (defaults to "Guild logo N"). */
  readonly label?: string;
}

/** The pixel size of the picture of `logo` at `scale`. */
export function logoPixels(logo: number, scale: number): number {
  return (logo >= 1 && logo <= GUILD_LOGO_COUNT ? guildLogoSize(logo) : GUILD_LOGO_SIZE) * scale;
}

export function GuildLogo({ logo, scale = 2, label }: GuildLogoProps) {
  const path = guildLogoPath(logo);
  const url = useAssetUrl(path ?? '');
  const px = logoPixels(logo, scale);
  const box = GUILD_LOGO_SIZE * scale;
  if (logo === 999) {
    return (
      <span className="adm-logo adm-logo--custom" data-logo={999} title={label ?? 'Custom logo'}>
        custom
      </span>
    );
  }
  if (path === null) {
    return (
      <span
        className="adm-logo adm-logo--none"
        data-logo={0}
        role="img"
        aria-label={label ?? 'No logo'}
        style={{ width: box, height: box }}
      />
    );
  }
  if (url === null) {
    return (
      <span
        className="adm-logo adm-logo--wait"
        data-logo={logo}
        role="img"
        aria-label={label ?? `Guild logo ${String(logo)}`}
        style={{ width: px, height: px }}
      />
    );
  }
  return (
    <img
      className="adm-logo"
      data-logo={logo}
      src={url}
      width={px}
      height={px}
      alt={label ?? `Guild logo ${String(logo)}`}
      draggable={false}
    />
  );
}
