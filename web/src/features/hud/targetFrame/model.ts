// The target frame (Plan/45 §7.3, ref/21 §6.8; `CWndWorld::RenderSelectObj`, `WndWorld.cpp:2725-2898` with `__ADDITIONAL_TARGET_INFO`
// and the vanilla frame `:2900-3119`; WP-CFEAT-09): the pure decision from a target to the lines, colours and icons of the frame.
// The DOM face is `TargetFrame.tsx`; the module (`targetModule.ts`) feeds the inputs.
//
// Geometry (virtual px of the world view): the gauge is 200 px wide, centred, its top at y = 30; the name sits at y - 18, the level at
// y + 12, the HP text centred at y - 3. The background fill is `ARGB(200,255,255,255)`, the HP fill `ARGB(128,255,15,15)`.

import {
  JOB_ELEMENTOR_HERO,
  JOB_ELEMENTOR_MASTER,
  JOB_KNIGHT_HERO,
  JOB_KNIGHT_MASTER,
} from '../../../gen/defs/job';

/** `AUTH_JOURNALIST` ('H') and `AUTH_GAMEMASTER` ('L'): `IsAuthHigher(x)` is `x <= authority`. */
export const AUTH_JOURNALIST = 72;
export const AUTH_GAMEMASTER = 76;

/** `COLOR_MONSTER`, `COLOR_PLAYER`, the aggressive monster and the HP text colours. */
export const COLOR_MONSTER = 0xffffffa0;
export const COLOR_PLAYER = 0xffffffff;
export const COLOR_AGGRESSIVE = 0xffff0000;
export const COLOR_HP_TEXT = 0xf0ffffff;
/** The bold name of the monster tooltip (`0xff2fbe6d`). */
export const COLOR_TOOLTIP_NAME = 0xff2fbe6d;

export const GAUGE_WIDTH = 200;
export const GAUGE_TOP = 30;
export const GAUGE_BACKGROUND = 0xc8ffffff;
export const GAUGE_FILL = 0x80ff0f0f;

/** `szElementString` and `szEffectiveElementString` (legacy English; **KEEP-QUIRK**: "Electirc" is the original's typo). */
export const ELEMENT_NAMES = ['None', 'Fire', 'Water', 'Electric', 'Wind', 'Earth'] as const;
export const EFFECTIVE_ELEMENT_NAMES = [
  'None',
  'Water',
  'Electirc',
  'Earth',
  'Fire',
  'Wind',
] as const;

/** `CMover::IsMaster()` / `IsHero()`: the job types 5 and 6 (the job ids 16..23 and 24..31). */
export const isMasterJob = (job: number): boolean =>
  job >= JOB_KNIGHT_MASTER && job <= JOB_ELEMENTOR_MASTER;
export const isHeroJob = (job: number): boolean =>
  job >= JOB_KNIGHT_HERO && job <= JOB_ELEMENTOR_HERO;

/** What the frame knows about its target. */
export interface TargetInput {
  readonly id: number;
  readonly player: boolean;
  /** `IsPeaceful()` (an NPC that cannot be attacked). */
  readonly peaceful: boolean;
  /** `TRANSPARENT_MODE` or the `SI_ACR_SUP_DARKILLUSION` buff. */
  readonly invisible: boolean;
  /** The authority letter of the target (players). */
  readonly authority: number;
  readonly name: string;
  readonly level: number;
  readonly job: number;
  readonly hp: number;
  readonly maxHp: number;
  /** `eElementType` of the monster prop (`0` none). */
  readonly element: number;
  /** `m_bActiveAttack` (an aggressive monster). */
  readonly activeAttack: boolean;
  /** The monster prop hides its level (`dwHideLevel == 1`, vanilla frame). */
  readonly hideLevel?: boolean;
}

export interface TargetEnv {
  /** The viewer's authority letter. */
  readonly viewerAuth: number;
  readonly viewerLevel: number;
  /** `features.additional_target_info`. */
  readonly additional: boolean;
  /** The width of the world view (the gauge is centred in it). */
  readonly width: number;
  /** The server texts of the GM line of the vanilla frame (`TID_GAME_SELECT_OBJECT_INFORMATION_*`). */
  readonly gmInfo?: (input: TargetInput) => string;
}

/** One text of the frame. */
export interface FrameText {
  readonly text: string;
  readonly x: number;
  readonly y: number;
  readonly color: number;
  /** Centred on `x` (the HP text). */
  readonly centered?: boolean;
}

export interface TargetFrameModel {
  readonly id: number;
  /** The gauge rect `x, y, 200` and the pixel width of the HP fill (i64 math: `200 * hp / maxHp`). */
  readonly gauge: {
    readonly x: number;
    readonly y: number;
    readonly width: number;
    readonly fill: number;
  };
  readonly name: FrameText;
  readonly level: FrameText;
  /** The HP text (monsters always; players only for own authority >= JOURNALIST); `null` = none. */
  readonly hp: FrameText | null;
  /** Job icon index (players) and element icon index (monsters with an element). */
  readonly jobIcon: number | null;
  readonly elementIcon: number | null;
  /** The monster tooltip over the 200 x 30 strip: bold name then the plain lines; `null` for players and the vanilla frame. */
  readonly tooltip: { readonly name: string; readonly lines: readonly string[] } | null;
  /** The GM line under the gauge (vanilla frame, viewer GM). */
  readonly gmLine: string | null;
}

/** `200 * hp / maxHp` in integer arithmetic (`(__int64)`), clamped into the gauge. */
export function gaugeFill(hp: number, maxHp: number): number {
  if (!(maxHp > 0)) return 0;
  const w = Math.trunc((GAUGE_WIDTH * hp) / maxHp);
  return Math.max(0, Math.min(GAUGE_WIDTH, w));
}

/** Is the target drawn at all (peaceful NPCs and, without the viewer being a game master, invisible movers are not). */
export function targetVisible(t: TargetInput, env: TargetEnv): boolean {
  if (t.peaceful && !t.player) return false;
  if (env.additional) {
    if (t.invisible) return env.viewerAuth >= AUTH_GAMEMASTER;
    return true;
  }
  // the vanilla frame also hides game masters from everybody but game masters
  const hidden = t.authority >= AUTH_GAMEMASTER || t.invisible;
  return !hidden || env.viewerAuth >= AUTH_GAMEMASTER;
}

/** The frame of a target; `null` = nothing is drawn. */
export function targetFrameModel(t: TargetInput, env: TargetEnv): TargetFrameModel | null {
  if (!targetVisible(t, env)) return null;
  const x = Math.trunc((env.width - GAUGE_WIDTH) / 2);
  const y = GAUGE_TOP;
  const gauge = { x, y, width: GAUGE_WIDTH, fill: gaugeFill(t.hp, t.maxHp) };
  return env.additional ? additionalFrame(t, env, x, y, gauge) : vanillaFrame(t, env, x, y, gauge);
}

function additionalFrame(
  t: TargetInput,
  env: TargetEnv,
  x: number,
  y: number,
  gauge: TargetFrameModel['gauge'],
): TargetFrameModel {
  const hpText = `${String(t.hp)} / ${String(t.maxHp)}`;
  if (t.player) {
    const staff = t.authority >= AUTH_JOURNALIST;
    const suffix = isHeroJob(t.job) ? ' - H' : isMasterJob(t.job) ? ' - M' : '';
    return {
      id: t.id,
      gauge,
      name: {
        text: staff ? `[Staff] ${t.name}` : t.name,
        x: x + 23,
        y: y - 18,
        color: COLOR_MONSTER,
      },
      level: { text: `Lv. ${String(t.level)}${suffix}`, x, y: y + 12, color: COLOR_MONSTER },
      // only staff sees the HP of a player
      hp: env.viewerAuth >= AUTH_JOURNALIST ? centred(hpText, env.width, y - 3) : null,
      jobIcon: t.job,
      elementIcon: null,
      tooltip: null,
      gmLine: null,
    };
  }
  const element = clampElement(t.element);
  const lines = [`Element : ${ELEMENT_NAMES[element] ?? 'None'}`];
  if (element > 0) lines.push(`Effective Element : ${EFFECTIVE_ELEMENT_NAMES[element] ?? 'None'}`);
  lines.push(`Level : ${String(t.level)}`);
  return {
    id: t.id,
    gauge,
    name: {
      text: t.name,
      x: x + (element > 0 ? 28 : 0),
      y: y - 18,
      color: t.activeAttack ? COLOR_AGGRESSIVE : COLOR_MONSTER,
    },
    level: { text: `Lv. ${String(t.level)}`, x, y: y + 12, color: COLOR_MONSTER },
    hp: centred(hpText, env.width, y - 3),
    jobIcon: null,
    elementIcon: element > 0 ? element - 1 : null,
    tooltip: { name: t.name, lines },
    gmLine: null,
  };
}

/** The vanilla frame: `Name <Lvl N>` at x + 50, no HP text, the GM line under the gauge. */
function vanillaFrame(
  t: TargetInput,
  env: TargetEnv,
  x: number,
  y: number,
  gauge: TargetFrameModel['gauge'],
): TargetFrameModel {
  let text: string;
  let color: number;
  let tx = x + 50;
  if (t.player) {
    // the level of another player is visible to game masters and when it is less than 20 above the viewer's own
    if (env.viewerAuth >= AUTH_GAMEMASTER || env.viewerLevel + 20 > t.level) {
      const tag = isHeroJob(t.job) ? '-H' : isMasterJob(t.job) ? '-M' : '';
      text = `${t.name} <Lvl ${String(t.level)}${tag}>`;
    } else {
      text = `${t.name} <Lvl ??>`;
    }
    color = COLOR_PLAYER;
    // centred in the 16-character name field (`__CSC_VER11_1`)
    const len = text.length;
    if (len <= 16) tx += (16 - len) * 3;
  } else {
    text = t.hideLevel === true ? t.name : `${t.name} <Lvl ${String(t.level)}>`;
    color = t.activeAttack ? COLOR_AGGRESSIVE : COLOR_MONSTER;
  }
  const element = t.player ? 0 : clampElement(t.element);
  const gm = env.viewerAuth >= AUTH_GAMEMASTER ? (env.gmInfo?.(t) ?? '') : '';
  return {
    id: t.id,
    gauge,
    name: { text, x: tx, y: y - 18, color },
    level: { text: '', x, y: y + 12, color: COLOR_MONSTER },
    hp: null,
    jobIcon: null,
    elementIcon: element > 0 ? element - 1 : null,
    tooltip: null,
    gmLine: gm === '' ? null : gm,
  };
}

const clampElement = (e: number): number => (e >= 0 && e < ELEMENT_NAMES.length ? e : 0);

function centred(text: string, width: number, y: number): FrameText {
  return { text, x: Math.trunc(width / 2), y, color: COLOR_HP_TEXT, centered: true };
}
