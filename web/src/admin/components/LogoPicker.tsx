// The visual logo picker (Plan/25 section 6.2 step 3, 14.2): a radio group of the 27 original guild logos at x3 (72 px cells, pixelated) plus
// a "None" tile; the logos 21 to 27 sit under the heading "GM logos (GM-only in the game)" (an administrator may choose them). Arrow keys move
// the choice (roving tabindex); the logos 22 and 27 are 20 x 20 pictures centred in their cell.
import { useRef, type KeyboardEvent } from 'react';
import { GUILD_LOGO_COUNT, GUILD_LOGO_GM_FROM } from '../../features/guild/logos';
import { GuildLogo } from './GuildLogo';

export interface LogoPickerProps {
  readonly value: number;
  readonly onChange: (logo: number) => void;
  readonly disabled?: boolean;
  /** The heading of the GM group. */
  readonly gmHeading?: string;
  readonly noneLabel?: string;
}

/** Every choice in keyboard order: 0 (none), then 1..27. */
export const LOGO_CHOICES: readonly number[] = Array.from(
  { length: GUILD_LOGO_COUNT + 1 },
  (_, i) => i,
);

/** The accessible name of a tile: "Logo 12", "GM logo 23", "No logo". */
export function logoLabel(logo: number): string {
  if (logo === 0) return 'No logo';
  return logo >= GUILD_LOGO_GM_FROM ? `GM logo ${String(logo)}` : `Logo ${String(logo)}`;
}

/** The choice an arrow key moves to (wraps around), `null` for any other key. */
export function nextLogo(current: number, key: string): number | null {
  const n = LOGO_CHOICES.length;
  const i = LOGO_CHOICES.indexOf(current);
  const at = i < 0 ? 0 : i;
  if (key === 'ArrowRight' || key === 'ArrowDown') return LOGO_CHOICES[(at + 1) % n] ?? null;
  if (key === 'ArrowLeft' || key === 'ArrowUp') return LOGO_CHOICES[(at - 1 + n) % n] ?? null;
  if (key === 'Home') return 0;
  if (key === 'End') return GUILD_LOGO_COUNT;
  return null;
}

export function LogoPicker({
  value,
  onChange,
  disabled = false,
  gmHeading = 'GM logos (GM-only in the game)',
  noneLabel = 'None',
}: LogoPickerProps) {
  const refs = useRef(new Map<number, HTMLButtonElement>());

  const onKeyDown = (event: KeyboardEvent): void => {
    const next = nextLogo(value, event.key);
    if (next === null) return;
    event.preventDefault();
    onChange(next);
    refs.current.get(next)?.focus();
  };

  const tile = (logo: number) => {
    const selected = logo === value;
    return (
      <button
        key={logo}
        ref={(el) => {
          if (el) refs.current.set(logo, el);
          else refs.current.delete(logo);
        }}
        type="button"
        role="radio"
        aria-checked={selected}
        aria-label={logoLabel(logo)}
        tabIndex={selected ? 0 : -1}
        disabled={disabled}
        className={`adm-logopick__tile ${selected ? 'is-selected' : ''} ${logo >= GUILD_LOGO_GM_FROM ? 'is-gm' : ''}`}
        data-logo={logo}
        onClick={() => {
          onChange(logo);
        }}
      >
        {logo === 0 ? (
          <span className="adm-logopick__none">{noneLabel}</span>
        ) : (
          <GuildLogo logo={logo} scale={3} label={logoLabel(logo)} />
        )}
      </button>
    );
  };

  const normal = LOGO_CHOICES.filter((l) => l >= 1 && l < GUILD_LOGO_GM_FROM);
  const gm = LOGO_CHOICES.filter((l) => l >= GUILD_LOGO_GM_FROM);
  return (
    <div className="adm-logopick" role="radiogroup" aria-label="Logo" onKeyDown={onKeyDown}>
      <div className="adm-logopick__grid">
        {tile(0)}
        {normal.map(tile)}
      </div>
      <h4 className="adm-logopick__gm">{gmHeading}</h4>
      <div className="adm-logopick__grid">{gm.map(tile)}</div>
    </div>
  );
}
