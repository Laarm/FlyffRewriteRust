// The character information page as pure functions (`CWndCharInfo::OnDraw`, `src:_Interface/WndField.cpp:3159-3750`,
// `OnInitialUpdate` :3750-3870, 45 §20.1; WP-UI-21): the geometry of both variants of the page (the plain one on
// `WndNewCharacter01.tga` and the taller one with the eight extra lines of `features.statistics` on `WndNewCharacter02.tga`),
// every text with its colour, the pending-allocation previews and the tooltips of the stat rows.

import {
  TID_APP_CHARACTER_BASE,
  TID_APP_CHARACTER_DETAIL,
  TID_APP_CHARACTER_JOB,
  TID_CRT_RATE,
  TID_GAME_CHARACTER_01,
  TID_GAME_CHARACTER_02,
  TID_GAME_CHARACTER_07,
  TID_GAME_CHARACTER_13,
  TID_GAME_CHARACTER_14,
  TID_GAME_CHARACTER_15,
  TID_GAME_CHARACTTER_PVP0,
  TID_GAME_CHARACTTER_PVP1,
  TID_GAME_CHARACTTER_PVP2,
  TID_GAME_CHARACTTER_PVP3,
  TID_GAME_CHARACTTER_PVP4,
  TID_GAME_CHAR_SERVER,
  TID_GAME_CHAR_SERVERNAME,
  TID_HIT_RATE,
  TID_M_BLOCK,
  TID_PVM_DMG,
  TID_PVP_DMG,
  TID_R_BLOCK,
  TID_SPD_RUN,
  TID_SPL_SPEED,
  TID_TOOLTIP_CHARSTATUS_EDIT,
  TID_TOOLTIP_CHARSTATUS_GPPOINT,
  TID_TOOLTIP_CHARSTATUS_STATUS1,
  TID_TOOLTIP_CHARSTATUS_STATUS2,
  TID_TOOLTIP_COMMAND_DEX0,
  TID_TOOLTIP_COMMAND_DEX1,
  TID_TOOLTIP_COMMAND_DEX2,
  TID_TOOLTIP_COMMAND_DEX3,
  TID_TOOLTIP_COMMAND_INT0,
  TID_TOOLTIP_COMMAND_INT1,
  TID_TOOLTIP_COMMAND_STA0,
  TID_TOOLTIP_COMMAND_STA1,
  TID_TOOLTIP_COMMAND_STR0,
  TID_TOOLTIP_COMMAND_STR1,
  TID_TOOLTIP_DEFENCE,
  TID_TOOLTIP_DEX,
  TID_TOOLTIP_INT,
  TID_TOOLTIP_STA,
  TID_TOOLTIP_STR,
} from '../../../gen/defs/text';
import type { TextItem } from '../../controls/TextLayer';
import type { Rect } from '../../layout/geometry';
import { argb, type RichRun } from '../../text/markup';
import type { CharacterView, DerivedStats, StatDelta, StatPair } from './ports';

/** `D3DCOLOR_ARGB(255, 0, 0, 0)`: values and headers. */
export const COLOR_VALUE = 0xff000000;
/** `D3DCOLOR_ARGB(255, 0, 0, 180)`: the captions in front of the values. */
export const COLOR_LABEL = 0xff0000b4;
/** The warning colour of a preview that differs from the current value (blinks), and of a lowered stat. */
export const COLOR_RED = 0xffff0000;
/** A raised stat (buffs, equipment). */
export const COLOR_BLUE = 0xff0000ff;

/** The two pictures of the page. */
export const CHAR_INFO_TEXTURE = 'WndNewCharacter01.tga';
export const CHAR_INFO_TEXTURE_STATISTICS = 'WndNewCharacter02.tga';

/** `(g_nRenderCnt / 8) & 1` at 60 frames per second: the warning colour flips every 8 frames (~133 ms). */
export const BLINK_MS = 133;
export function blinkOn(now: number): boolean {
  return (Math.floor(now / BLINK_MS) & 1) === 1;
}

/** The row layout of one variant (all numbers are the original's, in the page's own coordinates). */
export interface CharInfoGeometry {
  readonly statistics: boolean;
  readonly baseHeader: { x: number; y: number };
  readonly detailHeader: { x: number; y: number };
  readonly pvpHeader: { x: number; y: number };
  /** The first caption row of the information block (x 7) and the value column (x 80). */
  readonly infoY: number;
  /** The first row of the detail block and the rows' pitch. */
  readonly detailY: number;
  /** The first row of the stat block (STR) and where the last row (the points) is. */
  readonly statY: number;
  /** The edit, the buttons and the apply / reset buttons. */
  readonly edit: (row: number) => Rect;
  readonly plus: (row: number) => Rect;
  readonly minus: (row: number) => Rect;
  readonly apply: Rect;
  readonly reset: Rect;
  /** The y of the four value lines of the fame block and the captions' y. */
  readonly pvpValueY: readonly [number, number, number, number];
  readonly pvpLabelY: readonly [number, number, number, number];
}

const ROW = 15;
const POS_X = 128;

export function charInfoGeometry(statistics: boolean): CharInfoGeometry {
  const nyAdd = 121;
  const nyAdd2 = 284;
  const editY = statistics ? 102 + nyAdd : 49 + nyAdd;
  const statY = statistics ? 102 + nyAdd : 52 + nyAdd;
  const lastEditY = editY + 3 * ROW;
  const applyY = lastEditY + (statistics ? 136 : 36);
  const pvpValue0 = statistics ? 40 + nyAdd2 : 15 + nyAdd2;
  const pvpLabel0 = statistics ? 40 + nyAdd2 : 13 + nyAdd2;
  return {
    statistics,
    baseHeader: statistics ? { x: 56, y: 10 } : { x: 60, y: 13 },
    detailHeader: statistics ? { x: 56, y: 109 } : { x: 60, y: 113 },
    pvpHeader: statistics ? { x: 35, y: 302 } : { x: 60, y: 281 },
    infoY: 10 + 21,
    detailY: 10 + nyAdd,
    statY,
    edit: (row) => {
      const y = editY + row * ROW;
      return {
        l: POS_X - 38,
        t: statistics ? y - 2 : y,
        r: POS_X - 4,
        b: y + 16,
      };
    },
    plus: (row) => {
      const y = editY + row * ROW;
      return { l: POS_X, t: y + 2, r: POS_X + 14, b: y + 18 };
    },
    minus: (row) => {
      const y = editY + row * ROW;
      return { l: POS_X + 16, t: y + 2, r: POS_X + 30, b: y + 18 };
    },
    apply: { l: POS_X - 108, t: applyY, r: POS_X - 58, b: applyY + 22 },
    reset: { l: POS_X - 30, t: applyY, r: POS_X + 20, b: applyY + 22 },
    // fame name, fame, PK value, PK propensity (the plain page leaves a gap of 20 px before the PK value)
    pvpValueY: statistics
      ? [pvpValue0, pvpValue0 + 19, pvpValue0 + 38, pvpValue0 + 57]
      : [pvpValue0, pvpValue0 + 19, pvpValue0 + 19 + 35, pvpValue0 + 19 + 35 + 19],
    pvpLabelY: statistics
      ? [pvpLabel0, pvpLabel0 + 19, pvpLabel0 + 38, pvpLabel0 + 57]
      : [pvpLabel0, pvpLabel0 + 19, pvpLabel0 + 38 + 20, pvpLabel0 + 57 + 20],
  };
}

/** The master mark icon of a level range (`Icon_MasterMark1..6.dds`, 14 x 14) or the hero mark (44 x 14); `null` for neither. */
export function legendMark(
  legend: 0 | 1 | 2,
  level: number,
): { icon: string; x: number; y: number; w: number; h: number } | null {
  const y = 10 + 21 + 15 + 15 - 2;
  if (legend === 2) {
    return { icon: 'icon/icon_heromark.dds', x: 103, y, w: 44, h: 14 };
  }
  if (legend === 1) {
    let n = 0;
    if (level < 70) n = 1;
    else if (level < 80) n = 2;
    else if (level < 90) n = 3;
    else if (level < 100) n = 4;
    else if (level < 110) n = 5;
    else if (level <= 120) n = 6;
    if (n === 0) return null;
    return {
      icon: `icon/icon_mastermark${String(n)}.dds`,
      x: level < 100 ? 97 : 103,
      y,
      w: 14,
      h: 14,
    };
  }
  return null;
}

/** The colour of a stat value: black when the total equals the base, blue when higher (buffs), red when lower. */
export function statColor(s: StatPair): number {
  if (s.total === s.base) return COLOR_VALUE;
  return s.base < s.total ? COLOR_BLUE : COLOR_RED;
}

/** The pending allocation. */
export const NO_PENDING: StatDelta = { str: 0, sta: 0, dex: 0, int: 0 };
export function pendingSum(p: StatDelta): number {
  return p.str + p.sta + p.dex + p.int;
}

/** The derived values to show and whether each differs from the current ones (those blink). */
export interface DerivedShown {
  readonly stats: DerivedStats;
  readonly differs: Readonly<Record<keyof DerivedStats, boolean>>;
}

const DERIVED_KEYS: readonly (keyof DerivedStats)[] = [
  'atk',
  'def',
  'critical',
  'attackSpeed',
  'speedRun',
  'criticalRate',
  'spellCastSpeed',
  'pvmDmg',
  'hitRate',
  'blockMelee',
  'blockRange',
  'pvpDmg',
];

/**
 * `GetVirtualATK`, `GetVirtualDEF`, ... : with a pending allocation the values of the preview replace the current ones when
 * they differ - ATK only when STR, DEX or INT is pending, DEF only with STA, critical and attack speed only with DEX
 * (`RenderATK` and the checks of `OnDraw`); the statistics lines always compare.
 */
export function derivedShown(view: CharacterView, pending: StatDelta): DerivedShown {
  const cur = view.derived;
  const virt = pendingSum(pending) > 0 ? view.preview(pending) : null;
  const gate: Partial<Record<keyof DerivedStats, boolean>> = {
    atk: pending.str !== 0 || pending.dex !== 0 || pending.int !== 0,
    def: pending.sta !== 0,
    critical: pending.dex !== 0,
    attackSpeed: pending.dex !== 0,
  };
  const out: Record<string, number | undefined> = { ...cur };
  const differs = {} as Record<keyof DerivedStats, boolean>;
  for (const k of DERIVED_KEYS) {
    const v = virt?.[k];
    const c = cur[k];
    const allowed = gate[k] ?? true;
    const diff = virt !== null && allowed && v !== undefined && c !== undefined && v !== c;
    differs[k] = diff;
    if (diff) out[k] = v;
  }
  return { stats: out as unknown as DerivedStats, differs };
}

/** What the page prints. */
export interface CharInfoState {
  readonly view: CharacterView;
  readonly pending: StatDelta;
  /** `true` in the phase where the warning colour shows. */
  readonly blink: boolean;
  readonly getText: (tid: number) => string;
}

/** Every text of the page (`OnDraw`), in the original's order. */
export function charInfoTexts(s: CharInfoState): TextItem[] {
  const { view, getText } = s;
  const g = charInfoGeometry(view.statistics);
  const out: TextItem[] = [];
  const put = (x: number, y: number, text: string, color: number): void => {
    out.push({ x, y, text, color });
  };
  const warn = (differs: boolean): number => (differs && s.blink ? COLOR_RED : COLOR_VALUE);

  put(g.baseHeader.x, g.baseHeader.y, getText(TID_APP_CHARACTER_BASE), COLOR_VALUE);
  // the information block: values (x 80) and captions (x 7)
  let y = g.infoY;
  put(80, y, view.name, COLOR_VALUE);
  y += ROW;
  put(80, y, view.job, COLOR_VALUE);
  y += ROW;
  put(80, y, String(view.level), COLOR_VALUE);
  put(80, g.infoY + 45, view.server, COLOR_VALUE);
  put(80, g.infoY + 60, view.channel, COLOR_VALUE);
  const labelTids = [
    TID_GAME_CHARACTER_01,
    TID_APP_CHARACTER_JOB,
    TID_GAME_CHARACTER_02,
    TID_GAME_CHAR_SERVER,
    TID_GAME_CHAR_SERVERNAME,
  ];
  labelTids.forEach((tid, i) => {
    put(7, g.infoY + i * ROW, getText(tid), COLOR_LABEL);
  });

  // the detail block
  put(g.detailHeader.x, g.detailHeader.y, getText(TID_APP_CHARACTER_DETAIL), COLOR_VALUE);
  const shown = derivedShown(view, s.pending);
  const d = shown.stats;
  const df = shown.differs;
  const pct = (n: number | undefined): string => `${String(n ?? 0)}%`;
  const rowY = (row: number): number => g.detailY + row * ROW;
  put(50, rowY(0), String(d.atk), warn(df.atk));
  put(50, rowY(1), String(d.def), warn(df.def));
  put(140, rowY(0), pct(d.critical), warn(df.critical));
  put(140, rowY(1), pct(d.attackSpeed), warn(df.attackSpeed));
  put(7, rowY(0), getText(TID_GAME_CHARACTER_13), COLOR_LABEL);
  put(85, rowY(0), getText(TID_GAME_CHARACTER_14), COLOR_LABEL);
  put(7, rowY(1), getText(TID_TOOLTIP_DEFENCE), COLOR_LABEL);
  put(85, rowY(1), getText(TID_GAME_CHARACTER_15), COLOR_LABEL);
  if (view.statistics) {
    put(50, rowY(2), pct(d.speedRun), COLOR_VALUE);
    put(50, rowY(3), pct(d.criticalRate), COLOR_VALUE);
    put(50, rowY(4), pct(d.spellCastSpeed), COLOR_VALUE);
    put(50, rowY(5), pct(d.pvmDmg), COLOR_VALUE);
    put(140, rowY(2), pct(d.hitRate), warn(df.hitRate));
    put(140, rowY(3), pct(d.blockMelee), warn(df.blockMelee));
    put(140, rowY(4), pct(d.blockRange), warn(df.blockRange));
    put(140, rowY(5), pct(d.pvpDmg), warn(df.pvpDmg));
    const pairs: readonly [number, number][] = [
      [TID_SPD_RUN, TID_HIT_RATE],
      [TID_CRT_RATE, TID_M_BLOCK],
      [TID_SPL_SPEED, TID_R_BLOCK],
      [TID_PVM_DMG, TID_PVP_DMG],
    ];
    pairs.forEach(([l, r], i) => {
      put(7, rowY(2 + i), getText(l), COLOR_LABEL);
      put(85, rowY(2 + i), getText(r), COLOR_LABEL);
    });
  }

  // the stat block: values, captions, the free points
  const stats: readonly [StatPair, number][] = [
    [view.str, TID_TOOLTIP_STR],
    [view.sta, TID_TOOLTIP_STA],
    [view.dex, TID_TOOLTIP_DEX],
    [view.int, TID_TOOLTIP_INT],
  ];
  stats.forEach(([pair, tid], i) => {
    put(50, g.statY + i * ROW, String(pair.total), statColor(pair));
    put(7, g.statY + i * ROW, getText(tid), COLOR_LABEL);
  });
  const gp = view.remainGp - pendingSum(s.pending);
  put(105, g.statY + 4 * ROW, String(gp), gp !== 0 && s.blink ? COLOR_RED : COLOR_VALUE);
  put(7, g.statY + 4 * ROW, getText(TID_GAME_CHARACTER_07), COLOR_LABEL);

  // the fame block
  put(g.pvpHeader.x, g.pvpHeader.y, getText(TID_GAME_CHARACTTER_PVP0), COLOR_VALUE);
  const vy = g.pvpValueY;
  put(90, vy[0], view.fameName === '' ? '-' : view.fameName, COLOR_VALUE);
  put(90, vy[1], String(view.fame), COLOR_VALUE);
  put(90, vy[2], String(view.pkValue), COLOR_VALUE);
  put(90, vy[3], String(view.pkPropensity), COLOR_VALUE);
  const pvpTids = [
    TID_GAME_CHARACTTER_PVP1,
    TID_GAME_CHARACTTER_PVP2,
    TID_GAME_CHARACTTER_PVP3,
    TID_GAME_CHARACTTER_PVP4,
  ];
  pvpTids.forEach((tid, i) => {
    put(7, g.pvpLabelY[i] ?? 0, getText(tid), COLOR_LABEL);
  });
  return out;
}

// ---- tooltips (only the plain variant: the statistics build compiles them out, `#ifndef __STATISTICS`) ----------------------------

const NAME_COLOR = argb(255, 0, 93, 0);
const COMMAND_COLOR = argb(255, 180, 0, 0);

function run(text: string, color = COLOR_VALUE, bold = false): RichRun {
  return { text, color, bold, underline: false, strike: false };
}

export interface CharInfoTip {
  readonly rect: Rect;
  readonly runs: readonly RichRun[];
}

/** The hit rects of the stat rows and the other tooltip areas (`rectHittest[5]`, the status block, the edit column). */
export function charInfoTipRects(): {
  readonly rows: readonly Rect[];
  readonly status: Rect;
  readonly edits: Rect;
} {
  const nyAdd = 121;
  const rows: Rect[] = [
    { l: 10, t: 52 + nyAdd, r: 80, b: 65 + nyAdd },
    { l: 10, t: 67 + nyAdd, r: 80, b: 80 + nyAdd },
    { l: 10, t: 82 + nyAdd, r: 80, b: 95 + nyAdd },
    { l: 10, t: 97 + nyAdd, r: 80, b: 110 + nyAdd },
    { l: 10, t: 112 + nyAdd, r: 160, b: 125 + nyAdd },
  ];
  return {
    rows,
    status: { l: 7, t: 10 + nyAdd, r: 160, b: 38 + nyAdd },
    edits: { l: 90, t: 52 + nyAdd, r: 160, b: 110 + nyAdd },
  };
}

/** The tooltip under a point of the plain variant (`null` over no tooltip area or in the statistics variant). */
export function charInfoTip(
  pt: { x: number; y: number },
  view: CharacterView,
  pending: StatDelta,
  getText: (tid: number) => string,
): CharInfoTip | null {
  if (view.statistics) return null;
  const a = charInfoTipRects();
  const inside = (r: Rect): boolean => pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b;
  const rowIndex = a.rows.findIndex(inside);
  const row = a.rows[rowIndex];
  if (row) {
    const heading = (tid: number, value: number): RichRun[] => [
      run(getText(tid), NAME_COLOR, true),
      run(`  ${String(value)}\n `, NAME_COLOR, true),
    ];
    let runs: RichRun[];
    switch (rowIndex) {
      case 0:
        runs = [
          ...heading(TID_TOOLTIP_STR, view.str.total),
          run(getText(TID_TOOLTIP_COMMAND_STR0)),
          run('('),
          run(getText(TID_TOOLTIP_COMMAND_STR1), COMMAND_COLOR),
          run(')'),
        ];
        break;
      case 1:
        runs = [
          ...heading(TID_TOOLTIP_STA, view.sta.total),
          run(getText(TID_TOOLTIP_COMMAND_STA0)),
          run('\n '),
          run(getText(TID_TOOLTIP_COMMAND_STA1)),
        ];
        break;
      case 2:
        runs = [
          ...heading(TID_TOOLTIP_DEX, view.dex.total),
          run(getText(TID_TOOLTIP_COMMAND_DEX0)),
          run('\n '),
          run(getText(TID_TOOLTIP_COMMAND_DEX1)),
          run('\n '),
          run(getText(TID_TOOLTIP_COMMAND_DEX2)),
          run('\n '),
          run(getText(TID_TOOLTIP_COMMAND_DEX3), COMMAND_COLOR),
        ];
        break;
      case 3:
        runs = [
          ...heading(TID_TOOLTIP_INT, view.int.total),
          run(getText(TID_TOOLTIP_COMMAND_INT0)),
          run('\n '),
          run(getText(TID_TOOLTIP_COMMAND_INT1)),
        ];
        break;
      default:
        runs = [run(getText(TID_TOOLTIP_CHARSTATUS_GPPOINT))];
    }
    return { rect: row, runs };
  }
  if (inside(a.status)) {
    const any = pendingSum(pending) !== 0;
    return {
      rect: a.status,
      runs: [run(getText(any ? TID_TOOLTIP_CHARSTATUS_STATUS1 : TID_TOOLTIP_CHARSTATUS_STATUS2))],
    };
  }
  if (inside(a.edits)) {
    return { rect: a.edits, runs: [run(getText(TID_TOOLTIP_CHARSTATUS_EDIT))] };
  }
  return null;
}
