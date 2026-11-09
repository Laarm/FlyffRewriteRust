// The icons of the admin chrome: one 24x24 stroke path per name, drawn inline (no icon font, no extra request).
// Game visuals (item icons, logos, job icons) are separate components; this is the neutral admin set.
import type { SVGProps } from 'react';

const PATHS = {
  dashboard: 'M3 3h8v8H3zM13 3h8v5h-8zM13 10h8v11h-8zM3 13h8v8H3z',
  map: 'M9 4 3 6v14l6-2 6 2 6-2V4l-6 2zM9 4v14M15 6v14',
  users:
    'M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM2 21v-1a6 6 0 0 1 6-6h2a6 6 0 0 1 6 6v1M17 3.5a4 4 0 0 1 0 7.5M22 21v-1a6 6 0 0 0-4-5.6',
  package: 'M12 2 3 7v10l9 5 9-5V7zM3 7l9 5 9-5M12 12v10',
  shield: 'M12 2 4 5v6c0 5 3.5 9 8 11 4.500-2 8-6 8-11V5z',
  globe:
    'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3c2.500 2.500 3.500 5.500 3.500 9s-1 6.500-3.500 9c-2.500-2.500-3.500-5.500-3.500-9s1-6.500 3.500-9z',
  message: 'M4 4h16v12H8l-4 4z',
  server: 'M3 4h18v6H3zM3 14h18v6H3zM7 7h.01M7 17h.01',
  scroll: 'M8 3h11v15a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-2h10v2M8 3a3 3 0 0 0-3 3v10M11 8h5M11 12h5',
  search: 'M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM21 21l-5-5',
  bell: 'M6 9a6 6 0 0 1 12 0c0 7 3 8 3 8H3s3-1 3-8zM10 21h4',
  chevronDown: 'M6 9l6 6 6-6',
  chevronRight: 'M9 6l6 6-6 6',
  chevronLeft: 'M15 6l-6 6 6 6',
  x: 'M6 6l12 12M18 6 6 18',
  check: 'M5 12.500 10 17.500 19 7.500',
  alert: 'M12 3 2 20h20zM12 10v5M12 18h.01',
  info: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01',
  sun: 'M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM12 2v2M12 20v2M2 12h2M20 12h2M4.900 4.900l1.400 1.400M17.700 17.700l1.400 1.400M4.900 19.100l1.400-1.400M17.700 6.300l1.400-1.400',
  moon: 'M20 14.500A8 8 0 0 1 9.500 4 8 8 0 1 0 20 14.500z',
  monitor: 'M3 4h18v12H3zM8 21h8M12 16v5',
  user: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21v-1a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v1',
  key: 'M8 15a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM11 12h10M18 12v4M15 12v3',
  logout: 'M9 4H4v16h5M16 8l4 4-4 4M20 12H9',
  copy: 'M8 8h12v12H8zM4 16V4h12',
  pin: 'M12 17v5M8 3h8l-1 7 3 3v2H6v-2l3-3z',
  external: 'M14 4h6v6M20 4l-9 9M18 14v6H4V6h6',
  refresh: 'M20 12a8 8 0 1 1-2.500-5.800M20 4v5h-5',
  download: 'M12 3v12M7 10l5 5 5-5M4 21h16',
  filter: 'M3 5h18l-7 8v6l-4 2v-8z',
  sortAsc: 'M8 4v16M4 8l4-4 4 4M16 6h5M16 11h4M16 16h3',
  sortDesc: 'M8 20V4M4 16l4 4 4-4M16 18h5M16 13h4M16 8h3',
  menu: 'M4 6h16M4 12h16M4 18h16',
  lock: 'M6 11h12v10H6zM8 11V7a4 4 0 0 1 8 0v4',
  clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2',
  link: 'M10 14a4 4 0 0 0 5.700 0l3-3a4 4 0 0 0-5.700-5.700l-1 1M14 10a4 4 0 0 0-5.700 0l-3 3a4 4 0 0 0 5.700 5.700l1-1',
  sliders: 'M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0M14 4v4M8 10v4M16 16v4',
  help: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM9.500 9.500a2.500 2.500 0 1 1 3.500 2.300c-.700.400-1 1-1 1.700M12 17h.01',
  dot: 'M12 12h.01',
  sparkle: 'M12 3l2 6 6 2-6 2-2 6-2-6-6-2 6-2z',
} as const;

/** The names of the available icons. */
export type IconName = keyof typeof PATHS;

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'name' | 'children'> {
  readonly name: IconName;
  /** Pixel size (default 16). */
  readonly size?: number;
}

/** A decorative icon (`aria-hidden`); put the meaning in a label next to it or on the button around it. */
export function Icon({ name, size = 16, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
