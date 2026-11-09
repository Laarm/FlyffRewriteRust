// What the buff windows draw (Plan/45 §12, Plan/42 §19.5; WP-CFEAT-30), as pure functions of the buff sets, the props and the clock:
//
//   `buildHud`     the world strip (`CWndWorld::RenderBuff`): item, pet, equip and item2 buffs and the state-mode items on the RIGHT of the
//                  top centre, the party skills on the LEFT (the player's own skill buffs are the status window's); 37 px pitch starting
//                  at (width / 2 +- 112 / 100, 10), wrapping every `hudLimit(width)` icons by one row (45 px with timers, 37 without).
//                  The left (party) lane flows AROUND the rects of `HudInput.avoid` (the open skill-buff window, owner #004: no overlap):
//                  an icon whose box would overlap one starts the next row early; without `avoid` the output is the original's.
//   `buildStatus`  the skill buffs of `APP_BUFF_STATUS` (`CWndBuffStatus::OnDraw`), in the order of the buff map
//
// Two original quirks are kept: the x of the next icon advances even when the icon has no picture (and so draws nothing), and an
// experience scroll's icon wraps back to `width / 2 + 75` where the other item icons go back to `width / 2 + 112`.

import type { ClientItem } from '../../data/clientData';
import type { Rect } from '../../ui/layout/geometry';
import { II_PET_EGG, II_SYS_SYS_SCR_RETURN } from '../../gen/defs/item';
import { IK1_HOUSING, IK2_BUFF2, IK3_EGG, IK3_TICKET } from '../../gen/defs/itemkind';
import { DST_GIFTBOX } from '../../gen/defs/attribute';
import type { BuffHudIcon, BuffIcon } from '../../ui/windows/itemuse/ports';
import { type Buff, remainOf } from './buffSet';
import { spanOfMs, statusTimerBetter, statusTimerPlain, worldTimer, type TextOf } from './format';
import { expFamilyOf } from './ids';
import { isPenaltyKind, isResistKind, SM, SM_ITEM_ID, SM_MAX } from './sm';
import { BUFF_EQUIP, BUFF_ITEM, BUFF_PET, BUFF_SKILL } from './types';

/** `D3DCOLOR_XRGB(240, 240, 0)`: the world strip's timer. */
export const WORLD_TIMER_COLOR = 0xfff0f000;
/** The timer of the status window. */
export const STATUS_TIMER_COLOR = 0xffffffff;
/** `dwSkillTime` of a buff that never ends. */
export const PERMANENT_TIME = 999999999;

export interface DisplayEnv {
  text: TextOf;
  item(id: number): ClientItem | undefined;
  skill(id: number): ClientItem | undefined;
  partySkill(id: number): ClientItem | undefined;
  /** `features.better_buff_display`. */
  betterBuffDisplay(): boolean;
  /** `g_Option.m_bVisibleBuffTimeRender`. */
  timeRender(): boolean;
  /** The client clock (ms). */
  now(): number;
  /** The wall clock (unix seconds). */
  nowUnix(): number;
  /** The tier of a system pet's picture: 0, 1 or 2 (the egg icons come in three). */
  petIconTier?(itemId: number): number;
}

/** `GetBuffTimeGap`: the row pitch of the strip. */
export function buffTimeGap(timeRender: boolean): number {
  return timeRender ? 40 + 5 : 32 + 5;
}

/** `m_nLimitBuffCount`: the icons per row for the viewport width (the original has a table of its eight resolutions). */
export function hudLimit(width: number): number {
  if (width < 1024) return 7;
  if (width < 1280) return 10;
  if (width < 1360) return 13;
  if (width < 1400) return 14;
  if (width < 1600) return 15;
  if (width < 1680) return 17;
  return 18;
}

/** The picture of a buff: skills `icon/`, items `item/`, a gift box buff the gift box skill icon, pet eggs the tier file. */
export function buffIconPath(
  env: DisplayEnv,
  b: Pick<Buff, 'type' | 'id'>,
  party = false,
): string | null {
  if (b.type === BUFF_SKILL) {
    const p = party ? env.partySkill(b.id) : env.skill(b.id);
    return p && p.icon !== '' ? `icon/${p.icon}` : null;
  }
  const p = env.item(b.id);
  if (!p || p.icon === '') return null;
  if (p.dest_param.some((d) => d === DST_GIFTBOX)) return 'icon/Skill_TroGiftbox02.dds';
  if (b.type === BUFF_PET && p.item_kind3 === IK3_EGG && p.id !== II_PET_EGG) {
    const tier = env.petIconTier?.(p.id) ?? 0;
    return `item/${p.icon.replace('.', `_0${String(tier)}.`)}`;
  }
  return `item/${p.icon}`;
}

/** The prop a buff is described by. */
export function buffProp(
  env: DisplayEnv,
  b: Pick<Buff, 'type' | 'id'>,
  party = false,
): ClientItem | undefined {
  if (b.type === BUFF_SKILL) return party ? env.partySkill(b.id) : env.skill(b.id);
  return env.item(b.id);
}

/** The `type:id` key of a buff icon. */
export const buffIconKey = (b: Pick<Buff, 'type' | 'id'>, party = false): string =>
  `${party ? 'party:' : ''}${String(b.type)}:${String(b.id)}`;

// ---- the world strip -----------------------------------------------------------------------------------------------------------

interface Lane {
  x: number;
  y: number;
  /** The icons placed in the current row (the row wraps when it reaches the limit: the original's `count % limit`). */
  row: number;
  readonly delta: number;
}

/** What the strip is built from. */
export interface HudInput {
  /** The width of the layout rect (`m_rectWindow.Width()`). */
  readonly width: number;
  readonly own: readonly Buff[];
  /** The buffs of the party skills (`CWndWorld::m_buffs`): drawn on the left. */
  readonly party: readonly Buff[];
  /** The active premium timers: `kind` and the seconds left. */
  readonly sm: readonly { readonly kind: number; readonly remainSec: number }[];
  /**
   * Rects (strip client coordinates, the space of the icons' x / y) that the party lane keeps clear of: the open skill-buff window
   * (owner #004). Absent or empty = the original layout. The right lane never moves.
   */
  readonly avoid?: readonly Rect[];
}

/** The icon list of the world strip. */
export function buildHud(env: DisplayEnv, input: HudInput): BuffHudIcon[] {
  const half = Math.trunc(input.width / 2);
  const limit = hudLimit(input.width);
  const gap = buffTimeGap(env.timeRender());
  const out: BuffHudIcon[] = [];
  const left: Lane = { x: half - 100, y: 10, row: 0, delta: -(32 + 5) };
  const right: Lane = { x: half + 112, y: 10, row: 0, delta: 32 + 5 };
  const now = env.now();
  const avoid = input.avoid ?? [];

  const place = (lane: Lane, icon: BuffIcon, resetX: number): void => {
    out.push({ ...icon, x: lane.x, y: lane.y });
    lane.row += 1;
    if (lane.row >= limit) {
      lane.row = 0;
      lane.x = resetX;
      lane.y += gap;
    }
  };
  /** The party icon about to be placed at the lane's (x, y) would overlap a rect to avoid: the lane starts its next row early. */
  const clearOfAvoided = (lane: Lane): void => {
    const hits = (): boolean =>
      avoid.some((r) => lane.x < r.r && lane.x + 32 > r.l && lane.y < r.b && lane.y + gap > r.t);
    // every pass moves the lane one row down, so it ends when the row is below the lowest rect (or beside it)
    while (hits()) {
      lane.x = half - 100 + lane.delta;
      lane.y += gap;
      lane.row = 0;
    }
  };

  const expRendered = new Set<number>();
  for (const b of input.own) {
    if (b.type === BUFF_SKILL) continue; // the status window's
    const prop = env.item(b.id);
    if (
      b.type === BUFF_ITEM &&
      prop &&
      (prop.item_kind3 === IK3_TICKET || prop.item_kind1 === IK1_HOUSING)
    )
      continue;
    const family = expFamilyOf(b.id);
    if (family) {
      if (expRendered.has(family.kind)) continue;
      expRendered.add(family.kind);
      right.x += right.delta;
      const icon = expIcon(env, input.own, family.kind);
      if (icon) place(right, icon, half + 75);
      continue;
    }
    right.x += right.delta;
    const icon = hudIcon(env, b, false, now);
    if (icon) place(right, icon, b.type === BUFF_SKILL ? half - 100 : half + 112);
  }
  for (const b of input.party) {
    left.x += left.delta;
    const icon = hudIcon(env, b, true, now);
    if (icon) {
      clearOfAvoided(left);
      place(left, icon, half - 100);
    }
  }
  for (let kind = 0; kind < SM_MAX; kind += 1) {
    const rem = input.sm.find((s) => s.kind === kind)?.remainSec ?? 0;
    if (rem <= 0) continue;
    const itemId = SM_ITEM_ID[kind] ?? 0;
    const prop = itemId !== 0 ? env.item(itemId) : undefined;
    if (!prop || prop.icon === '' || isResistKind(kind)) continue;
    const ct = spanOfMs(rem * 1000);
    const shown = (ct.days === 0 && ct.hours === 0 && ct.minutes <= 30) || isPenaltyKind(kind);
    if (!shown) continue;
    right.x += 32 + 5;
    const flash = rem < 60 && kind !== SM.REVIVAL;
    const timer =
      prop.circle_time !== 1 && env.timeRender() ? worldTimer(ct, env.text) : { text: '', dx: 0 };
    place(
      right,
      {
        key: `sm:${String(kind)}`,
        icon: `item/${prop.icon}`,
        alpha: 192,
        flash,
        evil: false,
        size: 32,
        tipSize: 32,
        timer: timer.text,
        timerColor: WORLD_TIMER_COLOR,
        timerFont: 'text',
        timerShadow: false,
        timerDx: timer.dx,
        timerDy: 32,
      },
      half + 75,
    );
  }
  return out;
}

/** `RenderBuffIcon` of the world: one 32 x 32 icon; `null` when the buff has no picture (nothing is drawn, no slot used). */
function hudIcon(env: DisplayEnv, b: Buff, party: boolean, now: number): BuffIcon | null {
  const icon = buffIconPath(env, b, party);
  const prop = buffProp(env, b, party);
  if (!icon || !prop) return null;
  const remain = remainOf(b, now);
  // the return scroll always flashes (inside the timed branch of the original)
  const flash = b.total > 0 && (remain < 20_000 || b.id === II_SYS_SYS_SCR_RETURN);
  const timeText = hudTimer(env, b, prop, remain);
  return {
    key: buffIconKey(b, party),
    icon,
    alpha: 192,
    flash,
    evil: prop.evildoing < 0,
    size: 32,
    tipSize: 32,
    timer: timeText.text,
    timerColor: WORLD_TIMER_COLOR,
    timerFont: 'text',
    timerShadow: false,
    timerDx: timeText.dx,
    timerDy: 32,
  };
}

/** The timer under a world icon: the remaining time of a timed buff, the item2 expiry, else nothing. */
function hudTimer(
  env: DisplayEnv,
  b: Buff,
  prop: ClientItem,
  remain: number,
): { text: string; dx: number } {
  if (!env.timeRender()) return { text: '', dx: 0 };
  if (remain > 0 && prop.skill_time !== PERMANENT_TIME && b.type !== BUFF_EQUIP) {
    return worldTimer(spanOfMs(remain), env.text);
  }
  if (prop.item_kind2 === IK2_BUFF2) {
    return worldTimer(spanOfMs(Math.max(0, b.level - env.nowUnix()) * 1000), env.text);
  }
  return { text: '', dx: 0 };
}

/** `RenderExpBuffIcon`: ONE icon for the scrolls of a family, the second and third scroll as small copies under it. */
function expIcon(env: DisplayEnv, own: readonly Buff[], kind: number): BuffIcon | null {
  const family = expFamilyOf(own.find((b) => expFamilyOf(b.id)?.kind === kind)?.id ?? -1);
  if (!family) return null;
  const prop = env.item(family.base);
  if (!prop || prop.icon === '') return null;
  const count = family.overlap
    ? own.filter((b) => family.ids.includes(b.id)).length
    : Math.min(1, own.filter((b) => b.id === family.base).length);
  return {
    key: `exp:${String(kind)}`,
    icon: `item/${prop.icon}`,
    alpha: 192,
    flash: false,
    evil: prop.evildoing < 0,
    size: 32,
    tipSize: 32,
    timer: '',
    timerColor: WORLD_TIMER_COLOR,
    timerFont: 'text',
    timerShadow: false,
    timerDx: 0,
    timerDy: 32,
    extras: [
      ...(count >= 2 ? [{ dx: 5, dy: 30, size: 16 }] : []),
      ...(count >= 3 ? [{ dx: 22, dy: 30, size: 16 }] : []),
    ],
  };
}

// ---- the status window ---------------------------------------------------------------------------------------------------------

/** The skill buffs of `APP_BUFF_STATUS` (an icon without a picture is skipped, and so is its slot). */
export function buildStatus(env: DisplayEnv, own: readonly Buff[]): BuffIcon[] {
  const out: BuffIcon[] = [];
  const now = env.now();
  for (const b of own) {
    if (b.type !== BUFF_SKILL) continue;
    const icon = buffIconPath(env, b);
    const prop = env.skill(b.id);
    if (!icon || !prop) continue;
    const remain = b.total > 0 ? remainOf(b, now) : 0;
    const flash = b.total > 0 && remain < 20_000;
    let timer = '';
    if (b.total > 0 && env.timeRender()) {
      const ct = spanOfMs(remain);
      timer = env.betterBuffDisplay() ? statusTimerBetter(ct) : statusTimerPlain(ct);
    }
    const better = env.betterBuffDisplay();
    out.push({
      key: buffIconKey(b),
      icon,
      alpha: 192,
      flash,
      evil: prop.evildoing < 0,
      size: 38,
      tipSize: 28,
      timer,
      timerColor: STATUS_TIMER_COLOR,
      timerFont: better ? 'title' : 'text',
      timerShadow: !better,
      timerDx: 2,
      timerDy: better ? 20 : 22,
    });
  }
  return out;
}

/** The SM timers as the strip reads them: absolute kinds hold the unix expiry, the counter kinds the seconds left. */
const ABSOLUTE_KINDS = new Set<number>([
  SM.BANK15,
  SM.BANK30,
  SM.BANK001,
  SM.PARTYSKILL1,
  SM.PARTYSKILL15,
  SM.PARTYSKILL30,
  SM.SHOUT15,
  SM.SHOUT30,
  SM.SHOUT001,
  SM.ESCAPE,
]);

export function smRemaining(
  timers: readonly { readonly kind: number; readonly endUnix: number }[],
  nowUnix: number,
): { kind: number; remainSec: number }[] {
  const out: { kind: number; remainSec: number }[] = [];
  for (const t of timers) {
    const remainSec = ABSOLUTE_KINDS.has(t.kind) ? t.endUnix - nowUnix : t.endUnix;
    if (remainSec > 0) out.push({ kind: t.kind, remainSec: Math.trunc(remainSec) });
  }
  return out;
}
