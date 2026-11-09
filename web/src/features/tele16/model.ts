// The texts and the entries of the TELE16 window (`CWndTeleporter`, `src:_Interface/WndField.cpp:26711-26940`; Plan/45 §26, 15 §10.7;
// WP-CFEAT-29): the level requirement, the cooldown and the health lines of the boss panel, and the list of the named rows of
// `teleport16.inc` with the ROW index that `Tele16` carries. Pure functions: the module only reads the bundles.
//
// FIX (Plan/45 §26): the legacy printed `Health: ` with a stale buffer below 1 000 HP and sent the position of the list entry; here a
// small boss shows its plain health and the index sent is the row index of the data (they differ when a row has an empty name).

import type { ClientItem, ClientMover } from '../../data/clientData';
import type { Teleport16Row } from '../../gen/bundles';
import type { Tele16Boss, Tele16Entry } from '../../ui/windows/nav/ports';
import { itemView, plainItem } from '../inspect/items';

/** `MAX_LEVEL`: the last normal level; the legend range goes up to 129 (`H`). */
export const MAX_LEVEL = 120;

/** `Level Req.: ...`, or `Level Cap: N/A` for a row without a minimum level. */
export function levelRequirementText(min: number, max: number, cls: number): string {
  if (min === 0) return 'Level Cap: N/A';
  switch (cls) {
    case 1:
      // `max < MAX_LEVEL` M - M, `max > MAX_LEVEL` M - H; the legacy printed a stale buffer for `max == MAX_LEVEL`: here it is M - M
      return max > MAX_LEVEL
        ? `Level Req.: ${String(min)}M - ${String(max)}H`
        : `Level Req.: ${String(min)}M - ${String(max)}M`;
    case 2:
      return `Level Req.: ${String(min)}H - ${String(max)}H`;
    default:
      return max > MAX_LEVEL
        ? `Level Req.: ${String(min)} - ${String(max)}H`
        : `Level Req.: ${String(min)} - ${String(max)}`;
  }
}

/** `Cooldown: N Minutes` above 60 s (integer division), `N Seconds` for 1..60, `N/A` for 0. */
export function cooldownText(seconds: number): string {
  if (seconds <= 0) return 'Cooldown: N/A';
  if (seconds > 60) return `Cooldown: ${String(Math.trunc(seconds / 60))} Minutes`;
  return `Cooldown: ${String(seconds)} Seconds`;
}

/** `Health: N B` / `N M` / `N K` for 1e9 / 1e6 / 1e3 and more (integer division), the plain number below 1 000. */
export function healthText(addHp: number): string {
  if (addHp >= 1_000_000_000) return `Health:  ${String(Math.trunc(addHp / 1_000_000_000))} B`;
  if (addHp >= 1_000_000) return `Health:  ${String(Math.trunc(addHp / 1_000_000))} M`;
  if (addHp >= 1_000) return `Health:  ${String(Math.trunc(addHp / 1_000))} K`;
  return `Health:  ${String(addHp)}`;
}

export const levelLine = (level: number): string => `Level:  ${String(level)}`;
/** `Element: ` before the icon, `Element: N/A` without an element. */
export const elementLine = (element: number): string =>
  element > 0 ? 'Element: ' : 'Element: N/A';

export interface Tele16Lookups {
  mover(id: number): ClientMover | undefined;
  item(id: number): ClientItem | undefined;
}

/** The boss panel of a row, `null` when the row has no boss (or the mover is not in the data). */
export function bossOf(row: Teleport16Row, data: Tele16Lookups): Tele16Boss | null {
  if (row.boss === 0) return null;
  const m = data.mover(row.boss);
  if (!m) return null;
  return {
    name: m.name,
    levelText: levelLine(m.level),
    healthText: healthText(m.add_hp),
    element: m.element_type,
    elementText: elementLine(m.element_type),
    reqText: levelRequirementText(row.min_level, row.max_level, row.class),
    cooldownText: cooldownText(row.cooldown),
    drops: row.boss_drops.map((id) => {
      const prop = data.item(id);
      return {
        itemId: id,
        icon: prop && prop.icon !== '' ? `item/${prop.icon}` : null,
        view: itemView(prop, plainItem(id), `tele16:${String(id)}`),
      };
    }),
  };
}

/** The listed entries: every row with a non-empty name, each with its row index in the data. */
export function buildEntries(rows: readonly Teleport16Row[], data: Tele16Lookups): Tele16Entry[] {
  const out: Tele16Entry[] = [];
  rows.forEach((row, index) => {
    if (row.name.length === 0) return;
    out.push({ index, name: row.name, desc: row.desc, image: row.image, boss: bossOf(row, data) });
  });
  return out;
}
