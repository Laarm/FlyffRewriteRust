// The DPS meter model (`features.dps_meter`, Plan/45 §7.10, ref/17 §10; `src:_Interface/WndDPSMeter.cpp:14-221`; WP-CFEAT-36): the list of
// `DpsUpdate` as the rows the overlay draws. A pure function over plain data. The legacy window computes in C `float` and prints with
// `%.2f`; the arithmetic is rounded to single precision step by step (`Math.fround`) so that the strings equal the original's, including
// the KEEP-QUIRKs: a damage up to 1000 prints in "M" (`0.00M`), a damage above 2 100 000 000 prints 2100000000.00M undivided, a name shorter
// than three characters stops the drawing of the rows below it.

import {
  JOB_BILLPOSTER,
  JOB_BILLPOSTER_HERO,
  JOB_BILLPOSTER_MASTER,
  JOB_BLADE,
  JOB_BLADE_HERO,
  JOB_BLADE_MASTER,
  JOB_ELEMENTOR,
  JOB_ELEMENTOR_HERO,
  JOB_ELEMENTOR_MASTER,
  JOB_JESTER,
  JOB_JESTER_HERO,
  JOB_JESTER_MASTER,
  JOB_KNIGHT,
  JOB_KNIGHT_HERO,
  JOB_KNIGHT_MASTER,
  JOB_PSYCHIKEEPER,
  JOB_PSYCHIKEEPER_HERO,
  JOB_PSYCHIKEEPER_MASTER,
  JOB_RANGER,
  JOB_RANGER_HERO,
  JOB_RANGER_MASTER,
  JOB_RINGMASTER,
  JOB_RINGMASTER_HERO,
  JOB_RINGMASTER_MASTER,
  JOB_VAGRANT,
  JOB_ACROBAT,
  JOB_ASSIST,
  JOB_MAGICIAN,
  JOB_MERCENARY,
} from '../../../gen/defs/job';
import type { DpsEntry } from '../../pvp/store';

/** `MAX_DPSPLAYER`: only the first ten rows are drawn. */
export const DPS_MAX_PLAYERS = 10;
/** `PosY`: the top of the first row (the header sits above it). */
export const DPS_POS_Y = 145;
/** The width of the header, the body and the longest gauge. */
export const DPS_WIDTH = 280;
/** The row pitch. */
export const DPS_ROW_PITCH = 23;
/** The header height. */
export const DPS_HEADER_HEIGHT = 23;
/** The ARGB fill of the header and of the body. */
export const DPS_HEADER_ARGB = 0xb41e1e1e;
export const DPS_BODY_ARGB = 0x6e1e1e1e;
/** The ARGB of the title, of the names and of the values. */
export const DPS_TITLE_ARGB = 0xffeeeeee;
export const DPS_NAME_ARGB = 0xd2fffffa;
export const DPS_VALUE_ARGB = 0xffeeeeee;

/** `D3DCOLOR_ARGB(a, r, g, b)`. */
const argb = (a: number, r: number, g: number, b: number): number =>
  ((a << 24) | (r << 16) | (g << 8) | b) >>> 0;

const GREY = argb(140, 210, 210, 210);

/** The gauge colour of a job (`switch (pMover->GetJob())`); every master and hero shares the colour of the first class line. */
export function dpsJobColor(job: number): number {
  switch (job) {
    case JOB_VAGRANT:
      return argb(70, 189, 141, 70);
    case JOB_MERCENARY:
    case JOB_ACROBAT:
    case JOB_ASSIST:
    case JOB_MAGICIAN:
      return GREY;
    case JOB_KNIGHT:
    case JOB_KNIGHT_MASTER:
    case JOB_KNIGHT_HERO:
      return argb(140, 244, 163, 56);
    case JOB_BLADE:
    case JOB_BLADE_MASTER:
    case JOB_BLADE_HERO:
      return argb(140, 185, 18, 27);
    case JOB_JESTER:
    case JOB_JESTER_MASTER:
    case JOB_JESTER_HERO:
      return argb(140, 127, 198, 188);
    case JOB_RANGER:
    case JOB_RANGER_MASTER:
    case JOB_RANGER_HERO:
      return argb(140, 128, 196, 50);
    case JOB_RINGMASTER:
    case JOB_RINGMASTER_MASTER:
    case JOB_RINGMASTER_HERO:
      return argb(140, 255, 53, 139);
    case JOB_BILLPOSTER:
    case JOB_BILLPOSTER_MASTER:
    case JOB_BILLPOSTER_HERO:
      return argb(140, 255, 204, 51);
    case JOB_PSYCHIKEEPER:
    case JOB_PSYCHIKEEPER_MASTER:
    case JOB_PSYCHIKEEPER_HERO:
      return argb(140, 0, 204, 255);
    case JOB_ELEMENTOR:
    case JOB_ELEMENTOR_MASTER:
    case JOB_ELEMENTOR_HERO:
      return argb(140, 4, 99, 128);
    default:
      return GREY;
  }
}

const f32 = Math.fround;
/** What the MSVC cast `(int)NaN` gives (`0x80000000`): an all-zero list divides 0 by 0. */
const INT_MIN = -2147483648;

/**
 * The value text of one row: `"%.2f%c (%d%%)"`. `damage` is the message's `int` (a C `float` in the window), `total` the float sum of the
 * drawn rows. 500 -> `0.00M`, 1 500 -> `1.50k`, 2 500 000 -> `2.50M`, 2 147 483 647 -> `2100000000.00M`.
 */
export function formatDps(damage: number, total: number): string {
  const d = f32(damage);
  let value: number;
  let unit: string;
  if (d > 1000 && d < 1000000) {
    value = f32(d / 1000);
    unit = 'k';
  } else if (d > 2100000000) {
    value = 2100000000;
    unit = 'M';
  } else {
    value = f32(d / 1000000);
    unit = 'M';
  }
  const ratio = f32(f32(d / f32(total)) * 100);
  const percent = Number.isFinite(ratio) ? Math.trunc(ratio) : INT_MIN;
  return `${value.toFixed(2)}${unit} (${String(percent)}%)`;
}

/** The drawn name: more than 16 characters become the first 14 and `..`. */
export function dpsName(name: string): string {
  return name.length > 16 ? `${name.slice(0, 14)}..` : name;
}

/** The title of the header. */
export function dpsTitle(count: number): string {
  return `DPS Calculation (${String(Math.min(count, DPS_MAX_PLAYERS))}/${String(DPS_MAX_PLAYERS)} Players)`;
}

/** One drawn row (positions in the pixels of the overlay). */
export interface DpsRow {
  /** The top of the row (`PosY`). */
  readonly y: number;
  readonly name: string;
  readonly value: string;
  /** The gauge rectangle `(1, y + 2, 1 + width, y + 22)`; `null` when the player is not in view (the original paints it for visible players only). */
  readonly gauge: { readonly width: number; readonly color: number } | null;
}

export interface DpsModel {
  readonly title: string;
  /** The body rectangle height: `n * 23` over ALL rows of the list, also those that are not drawn. */
  readonly bodyHeight: number;
  readonly rows: readonly DpsRow[];
}

/** What the model needs to know of the client. */
export interface DpsEnv {
  /** The job of a player in view (`prj.GetUserByID`), `null` when the player is not in view. */
  jobOf(idPlayer: number): number | null;
}

/**
 * The overlay model: `null` unless the meter is shown (`m_bShowDPS`) and the list is not empty (`m_aDPSPlayer.size() == 0`). The entries
 * are sorted by damage, descending, already.
 */
export function dpsModel(
  entries: readonly DpsEntry[],
  show: boolean,
  env: DpsEnv,
): DpsModel | null {
  if (!show || entries.length === 0) return null;
  const drawn = Math.min(entries.length, DPS_MAX_PLAYERS);
  let total = 0;
  for (let i = 0; i < drawn; i += 1) total = f32(total + f32(entries[i]?.damage ?? 0));
  const first = f32(entries[0]?.damage ?? 0);
  const rows: DpsRow[] = [];
  let y = DPS_POS_Y;
  for (let i = 0; i < drawn; i += 1) {
    const e = entries[i];
    if (!e) break;
    const d = f32(e.damage);
    const value = formatDps(e.damage, total);
    // `m_fGauge`: this damage over the first one; a row without damage has an empty gauge
    let g = d === 0 ? 0 : f32(d / first);
    if (Number.isNaN(g)) g = 0;
    // a name of less than three characters ends the drawing (`return` inside the loop)
    if (e.name.length < 3) break;
    let width = 1 + Math.trunc(f32(f32(DPS_WIDTH - 1) * f32(g)));
    width = width < DPS_WIDTH ? width : DPS_WIDTH;
    const job = env.jobOf(e.idPlayer);
    rows.push({
      y,
      name: dpsName(e.name),
      value,
      gauge: job === null ? null : { width, color: dpsJobColor(job) },
    });
    y += DPS_ROW_PITCH;
  }
  return { title: dpsTitle(entries.length), bodyHeight: entries.length * DPS_ROW_PITCH, rows };
}

/** The entries of a message sorted by damage, descending (`sort_dpsmeter`: `first.fDamage > second.fDamage`). */
export function sortDps(entries: readonly DpsEntry[]): DpsEntry[] {
  return [...entries].sort((a, b) => b.damage - a.damage);
}
