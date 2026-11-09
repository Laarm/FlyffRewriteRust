// The HUD of the siege as data (Plan/45 §28.4, §7.11; `CWndWorld::OnDraw`, `src:_Interface/WndWorld.cpp:1140-1760`): the timer line, the two boards at the right edge (the siege world only), the
// roster of the own guild at the left edge (any world) and the "be ready" lines. Pure: `hudModel( slice, timer, env )` gives rectangles and texts in the virtual pixels of the UI; `SiegeHud.tsx`
// only places them. The numbers are the original's:
//
//   timer           centred at x = width / 2, y = 110, white with a `0xff990099` shadow: "<label> HH:MM:SS" (or "<label> <WAITSEC( n )>"); gone when it reaches 0
//   guild board     origin (width - 150, 200): the rect from (x - 10, y - 10), 150 wide, (rows + 3) * 16 high, black alpha 30; header at x + 10; rows from y + 24, 16 apart
//   person board    50 px below the last guild row, the same geometry; the top ten rows, the own row appended after a gap when it is not among them
//   roster          the rect (5, 120), 135 wide, (rows + 2) * 16 high; the guild name at (10, 129); rows from y = 150, 16 apart; the HP bars are 60 x 11
//
// The rows are drawn in the 12 px text font (their pitch of 16 px is that of the 12 px cell); the timer and the "be ready" lines use `FONT.guildCombat` (17 px), the font the original
// sets for exactly those two.

import {
  TID_GAME_GUILDCOMBAT_1TO1_WAITSEC,
  TID_GAME_GUILDCOMBAT_JOIN_READY,
  TID_GAME_GUILDCOMBAT_JOIN_READY2,
  TID_GAME_GUILDCOMBAT_PERSON_RATE,
  TID_GAME_GUILDCOMBAT_RATE,
} from '../../../gen/defs/text';
import { cutName, clock, padLeft, timeSpan } from '../../../ui/windows/siege/format';
import { type HudTimer, SIEGE_STATE, type SiegeSlice, WORLD_GUILDWAR } from '../store';
import { guildBoard, personBoard } from './ranking';

/** One piece of text: the top-left of its box (or the centre line with `center`), `0xAARRGGBB`. */
export interface HudText {
  readonly x: number;
  readonly y: number;
  readonly text: string;
  readonly color: number;
  /** The colour of the 1 px shadow (`TextOut( ..., color, shadow )`). */
  readonly shadow?: number;
  /** `x` is the centre of the text. */
  readonly center?: boolean;
  /** The big font of the siege (`FONT.guildCombat`). */
  readonly big?: boolean;
}

export interface HudRect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
  readonly color: number;
}

export interface HudPanel {
  readonly rect: HudRect;
  readonly texts: readonly HudText[];
}

export interface RosterPanel extends HudPanel {
  /** The HP bars: a black plate and the green fill of each row. */
  readonly bars: readonly HudRect[];
}

export interface SiegeHudModel {
  readonly timer: HudText | null;
  readonly guildBoard: HudPanel | null;
  readonly personBoard: HudPanel | null;
  readonly roster: RosterPanel | null;
  readonly joinReady: readonly HudText[];
}

export interface HudEnv {
  /** The virtual viewport. */
  readonly width: number;
  readonly height: number;
  /** `ctx.now()` in ms. */
  readonly now: number;
  readonly worldId: number;
  readonly selfCharId: number;
  /** The own guild's name, `''` without a guild. */
  readonly selfGuildName: string;
  /** The name of a character now known to the cache. */
  nameOf(id: number): string;
  /** The HP of a member as a fraction (0 when he is not in view). */
  hpOf(id: number): number;
  /** `prj.GetText`. */
  text(tid: number): string;
  /** `GetText( tid )` filled with one number. */
  fmt(tid: number, n: number): string;
  /** The own character's name. */
  selfName: string;
}

export const EMPTY_HUD: SiegeHudModel = {
  timer: null,
  guildBoard: null,
  personBoard: null,
  roster: null,
  joinReady: [],
};

const WHITE = 0xffffffff;
const BLACK = 0xff000000;
const HEADER = 0xffebad18;
const OWN = 0xff9ed3ff;
const OTHER = 0xffffff99;
const GREEN = 0xff00cb00;
const GREY = 0xffcbcbcb;
const YELLOW = 0xffffff00;
const TIMER_SHADOW = 0xff990099;
const BOARD_ALPHA = 0x1e000000;
const BAR_ALPHA = 0x64000000;
/** `nGap`. */
const GAP = 16;
/** The cell height of `FONT.guildCombat` (`GetTextExtent( ... ).cy`). */
const BIG_CELL = 21;

/** The seconds left of a timer, never below 0. */
export function secondsLeft(timer: HudTimer, now: number): number {
  return Math.max(0, Math.floor((timer.endMs - now) / 1000));
}

/** The timer line, `null` when no timer runs or it reached 0 (the module clears it then). */
export function timerText(timer: HudTimer | null, env: HudEnv): HudText | null {
  if (timer === null) return null;
  const left = secondsLeft(timer, env.now);
  if (left <= 0) return null;
  const text =
    timer.waitSec === true
      ? `${timer.label} ${env.fmt(TID_GAME_GUILDCOMBAT_1TO1_WAITSEC, timeSpan(left).seconds)}`
      : `${timer.label} ${clock(left)}`;
  return {
    x: Math.trunc(env.width / 2),
    y: 110,
    text,
    color: WHITE,
    shadow: TIMER_SHADOW,
    center: true,
    big: true,
  };
}

const board = (
  x: number,
  y: number,
  rows: number,
  header: string,
  lines: readonly HudText[],
): HudPanel => ({
  rect: { x: x - 10, y: y - 10, w: 150, h: (rows + 3) * GAP, color: BOARD_ALPHA },
  texts: [{ x: x + 10, y, text: header, color: HEADER, shadow: BLACK }, ...lines],
});

/** One row of a board: the rank (blank on a tie), the name (5 characters and dots) and the points. */
function boardRow(
  x: number,
  y: number,
  rank: number,
  tie: boolean,
  name: string,
  points: number,
  color: number,
): HudText[] {
  return [
    { x, y, text: tie ? '  ' : padLeft(rank, 2), color, shadow: BLACK },
    { x: x + 25, y, text: cutName(name, 5), color, shadow: BLACK },
    { x: x + 110, y, text: String(points), color, shadow: BLACK },
  ];
}

/** The two boards of the siege world. */
function boards(slice: SiegeSlice, env: HudEnv): { guild: HudPanel; person: HudPanel } {
  const x = env.width - 150;
  let y = 200;
  const own = env.selfGuildName.toLowerCase();
  const guilds = guildBoard(slice.guildRank);
  const guildLines: HudText[] = [];
  y += GAP + GAP / 2;
  for (const g of guilds) {
    const color = own !== '' && g.item.name.toLowerCase() === own ? OWN : OTHER;
    guildLines.push(...boardRow(x, y, g.rank, !g.first, g.item.name, g.item.points, color));
    y += GAP;
  }
  const guild = board(x, 200, guilds.length, env.text(TID_GAME_GUILDCOMBAT_RATE), guildLines);

  // the person board starts 50 px below the last guild row
  y += 50;
  const top = y;
  const people = personBoard(slice.playerRank);
  const lines: HudText[] = [];
  y += GAP + GAP / 2;
  let drawn = 0;
  let selfRank = 0;
  let selfPoints = 0;
  let selfShown = false;
  for (const p of people) {
    drawn += 1;
    if (drawn > 10) {
      if (p.item.id === env.selfCharId) {
        selfRank = p.rank;
        selfPoints = p.item.points;
        selfShown = false;
        break;
      }
      continue;
    }
    if (p.item.id === env.selfCharId) selfShown = true;
    const color = p.item.id === env.selfCharId ? OWN : OTHER;
    lines.push(...boardRow(x, y, p.rank, !p.first, env.nameOf(p.item.id), p.item.points, color));
    y += GAP;
  }
  if (people.length > 0 && !selfShown) {
    y += GAP;
    lines.push(...boardRow(x, y, selfRank, false, env.selfName, selfPoints, OWN));
  }
  const person = board(x, top, people.length, env.text(TID_GAME_GUILDCOMBAT_PERSON_RATE), lines);
  return { guild, person };
}

/** The roster of the own guild with its lives and HP bars; `null` unless the own character is in it. */
function rosterPanel(slice: SiegeSlice, env: HudEnv): RosterPanel | null {
  if (env.selfGuildName === '') return null;
  const inRoster = slice.roster.some((g) => g.players.some((p) => p.id === env.selfCharId));
  if (!inRoster) return null;
  const defenders = new Set(slice.roster.map((g) => g.defender));
  const x = 10;
  let y = 150;
  const texts: HudText[] = [
    { x, y: y - (GAP + 5), text: env.selfGuildName, color: HEADER, shadow: BLACK },
  ];
  const bars: HudRect[] = [];
  let joinReady = false;
  let space = false;
  slice.ownLives.forEach((row, k) => {
    const isReady = slice.joinIndex !== 0 && k === slice.joinIndex;
    let color = row.id === env.selfCharId ? OWN : OTHER;
    // the next one to enter: a blank row above, green
    if (isReady && row.lives > 0) {
      y += GAP;
      joinReady = true;
    }
    // the players with no life left: a blank row above, grey
    if (row.lives <= 0 && !space) {
      y += GAP;
      space = true;
    }
    if (joinReady || space) {
      if (joinReady) {
        color = GREEN;
        joinReady = false;
      } else {
        color = GREY;
      }
    }
    if (defenders.has(row.id)) texts.push({ x: x - 6, y, text: 'D', color, shadow: BLACK });
    texts.push({ x, y, text: padLeft(k + 1, 2), color, shadow: BLACK });
    texts.push({ x: x + 25, y, text: cutName(env.nameOf(row.id), 5), color, shadow: BLACK });
    texts.push({ x: x + 110, y, text: String(Math.max(0, row.lives - 1)), color, shadow: BLACK });
    // the HP bar: a black plate of 60 x 11 and the green fill inside it
    const hp = Math.max(0, Math.min(1, env.hpOf(row.id)));
    const full = 60;
    const drawn = full * hp;
    bars.push({ x: x + 140, y, w: full, h: 11, color: BAR_ALPHA });
    const fill = full - Math.trunc(full - drawn + 1) - 1;
    if (fill > 0) bars.push({ x: x + 141, y: y + 1, w: fill, h: 9, color: 0x6400ff00 });
    y += GAP;
  });
  return {
    rect: { x: 5, y: 120, w: 135, h: (slice.ownLives.length + 2) * GAP, color: BOARD_ALPHA },
    texts,
    bars,
  };
}

/** The "be ready" lines: the own row is the next to enter and the war is on. */
function joinReadyLines(slice: SiegeSlice, env: HudEnv): HudText[] {
  if (slice.state !== SIEGE_STATE.War) return [];
  const k = slice.ownLives.findIndex((r) => r.id === env.selfCharId);
  const row = slice.ownLives[k];
  if (!row || slice.joinIndex === 0 || k !== slice.joinIndex || row.lives <= 0) return [];
  const cx = Math.trunc(env.width / 2);
  const y = Math.trunc(env.height / 2) - 150;
  return [
    {
      x: cx,
      y,
      text: env.text(TID_GAME_GUILDCOMBAT_JOIN_READY),
      color: YELLOW,
      shadow: BLACK,
      center: true,
      big: true,
    },
    {
      x: cx,
      y: Math.trunc(env.height / 2) - 148 + BIG_CELL,
      text: env.text(TID_GAME_GUILDCOMBAT_JOIN_READY2),
      color: YELLOW,
      shadow: BLACK,
      center: true,
      big: true,
    },
  ];
}

/** The whole HUD for a state of the siege. */
export function hudModel(slice: SiegeSlice, timer: HudTimer | null, env: HudEnv): SiegeHudModel {
  const inSiegeWorld = env.worldId === WORLD_GUILDWAR;
  const b = inSiegeWorld ? boards(slice, env) : null;
  return {
    timer: timerText(timer, env),
    guildBoard: b?.guild ?? null,
    personBoard: b?.person ?? null,
    roster: rosterPanel(slice, env),
    joinReady: joinReadyLines(slice, env),
  };
}
