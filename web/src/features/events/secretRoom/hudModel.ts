// The war board of the secret room as data (Plan/45 §28.6, §7.11; `CWndWorld::DrawSecretRoomInfo` and its helpers, `src:_Interface/WndWorld.cpp:1774-2190`): the guilds of the continent as rows at the left
// edge, sorted by war state and kills. Pure: `hudBoard( slice, env )` gives rectangles and texts in the virtual pixels of the UI; `SecretRoomHud.tsx` only places them and plays the animations.
//
//   board         starts at crBoard = (20, 120, 290, 140); drawn only while the state is 103 or more, the player stands in a secret room and has a guild
//   sort          war state descending, then kills descending (`prKCountAsce`; STABLE for ties, FIX of the unspecified `std::sort`)
//   own guild     rect (10, crBoard.top + 18, 315, +25) - `crBoard.top += 15` - text origin (16, rectTop + 4), logo (36, rectTop) scale 1.0, name +50, separator +210, state +230, `FONT.secretRoom`
//   other guild   `crBoard.top += 25`, rect (20, top, 290, top + 20), text origin (24, top + 4), logo (38, top) scale 0.8, name +36, separator +180, state +200, a 9-pt bold outline-1 font
//   an unknown guild is skipped (`GetGuild == NULL`) but keeps its number: the rank text is "%d." of the index in the sorted list + 1 (KEEP)

import {
  TID_GAME_SECRETROOM_FIN_BOSS,
  TID_GAME_SECRETROOM_LOSS,
  TID_GAME_SECRETROOM_MID_BOSS,
  TID_GAME_SECRETROOM_WIN,
} from '../../../gen/defs/text';
import { cutOver } from '../../../ui/windows/secretroom/rows';
import type { SecretRoomSlice } from '../store';
import { SR_WAR, SR_WAR_STATE } from './constants';

export { SR_WAR_STATE };

/** One piece of text: the top-left of its box, `0xAARRGGBB`. */
export interface BoardText {
  readonly x: number;
  readonly y: number;
  readonly text: string;
  readonly color: number;
}

export interface BoardRect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** The pulsing outline of a guild that reached the middle boss, the boss or won (`DrawOutLineLamp`). */
export interface BoardLamp extends BoardRect {
  /** 4 for the own guild, 2 for the others. */
  readonly thick: number;
  /** `0xRRGGBB` of the state. */
  readonly rgb: number;
  /** Frames per half period at 60 fps: 25 in MidBoss and Boss (step 0.04), 100 in Win (step 0.01). */
  readonly halfFrames: number;
}

export interface BoardRow {
  /** The index in the sorted list (the number printed is `index + 1`). */
  readonly index: number;
  readonly own: boolean;
  readonly guildId: number;
  readonly war: number;
  readonly kills: number;
  readonly rect: BoardRect;
  /** `0xAARRGGBB`. */
  readonly fill: number;
  readonly rank: BoardText;
  /** The logo number (> 0) and its place and scale, `null` without one. */
  readonly logo: {
    readonly logo: number;
    readonly x: number;
    readonly y: number;
    readonly scale: number;
  } | null;
  readonly name: BoardText;
  readonly sep: BoardText;
  /** The state text of WIN / LOSS / MID BOSS / FIN BOSS, `null` in NORMAL. */
  readonly state: BoardText | null;
  /** The kill counter of a NORMAL guild: "%03d", "/", "%03d". The own guild draws the three digits as cells of 9 px. */
  readonly count: {
    readonly digits: readonly BoardText[];
    readonly slash: BoardText;
    readonly total: BoardText;
  } | null;
  readonly lamp: BoardLamp | null;
}

export interface HudBoard {
  readonly rows: readonly BoardRow[];
}

/** What the model reads besides the slice. */
export interface BoardEnv {
  readonly inRoom: boolean;
  readonly ownGuildId: number;
  /** The name of any known guild, `null` when unknown. */
  guildName(id: number): string | null;
  /** The logo number of a guild, 0 for none. */
  logoOf(id: number): number;
  text(tid: number): string;
}

const WHITE = 0xffffffff;
const NAME = 0xfffebb1b;
const SLASH = 0xffc2d1ff;
const FILL_FAILED = 0x96ff0000;
const FILL_OWN = 0x640064ff;
const FILL_OTHER = 0x64f5ccb0;
/** `MAX_KILLCOUNT_CIPHERS`. */
export const KILL_CIPHERS = 3;

/** `"%03d"`. */
export const pad3 = (n: number): string => String(Math.max(0, Math.trunc(n))).padStart(3, '0');

/** The sorted order of the guilds (`prKCountAsce`, stable). */
export function sortedGuilds(slice: SecretRoomSlice): SecretRoomSlice['guilds'][number][] {
  return slice.guilds
    .map((g, i) => ({ g, i }))
    .sort((a, b) => b.g.war - a.g.war || b.g.kills - a.g.kills || a.i - b.i)
    .map((x) => x.g);
}

/** The war-state parts: the text of the state column and the lamp colour. */
function stateOf(
  war: number,
  text: (tid: number) => string,
): { text: string; color: number; rgb: number; halfFrames: number } | null {
  switch (war) {
    case SR_WAR.Win:
      return {
        text: text(TID_GAME_SECRETROOM_WIN),
        color: 0xffffff00,
        rgb: 0xffff00,
        halfFrames: 100,
      };
    case SR_WAR.Failed:
      return { text: text(TID_GAME_SECRETROOM_LOSS), color: 0xffff0000, rgb: -1, halfFrames: 0 };
    case SR_WAR.MidBoss:
      return {
        text: text(TID_GAME_SECRETROOM_MID_BOSS),
        color: 0xff8be3e2,
        rgb: 0x8be3e2,
        halfFrames: 25,
      };
    case SR_WAR.Boss:
      return {
        text: text(TID_GAME_SECRETROOM_FIN_BOSS),
        color: 0xfff67608,
        rgb: 0xf67608,
        halfFrames: 25,
      };
    default:
      return null;
  }
}

/** The board; `null` when it is not drawn (before the war, outside a room, without a guild). */
export function hudBoard(slice: SecretRoomSlice, env: BoardEnv): HudBoard | null {
  if (slice.state < SR_WAR_STATE || !env.inRoom || env.ownGuildId === 0) return null;
  const rows: BoardRow[] = [];
  // crBoard.top
  let boardTop = 120;
  sortedGuilds(slice).forEach((g, j) => {
    const guildName = env.guildName(g.guildId);
    // an unknown guild is skipped and takes no space, but its number is gone
    if (guildName === null) return;
    const own = g.guildId === env.ownGuildId;
    let rect: BoardRect;
    let cx: number;
    let cy: number;
    let logoAt: { x: number; y: number };
    let nameDx: number;
    let sepDx: number;
    let stateDx: number;
    let scale: number;
    if (own) {
      const top = boardTop + 18;
      boardTop += 15;
      rect = { x: 10, y: top, w: 305, h: 25 };
      cx = 16;
      cy = top + 4;
      logoAt = { x: 36, y: top };
      [nameDx, sepDx, stateDx, scale] = [50, 210, 230, 1];
    } else {
      boardTop += 25;
      rect = { x: 20, y: boardTop, w: 270, h: 20 };
      cx = 24;
      cy = boardTop + 4;
      logoAt = { x: 38, y: boardTop };
      [nameDx, sepDx, stateDx, scale] = [36, 180, 200, 0.8];
    }
    const failed = g.war === SR_WAR.Failed;
    const fill = failed ? FILL_FAILED : own ? FILL_OWN : FILL_OTHER;
    const fight = g.war === SR_WAR.MidBoss || g.war === SR_WAR.Boss;
    const total = slice.totals.get(g.war) ?? 0;
    const st = stateOf(g.war, (tid) => env.text(tid));
    const sx = cx + stateDx;
    let count: BoardRow['count'] = null;
    if (g.war === SR_WAR.Normal) {
      if (own) {
        count = {
          digits: Array.from({ length: KILL_CIPHERS }, (_, i) => ({
            x: sx + 9 * i,
            y: cy,
            text: pad3(g.kills).charAt(i),
            color: WHITE,
          })),
          slash: { x: sx + 9 * KILL_CIPHERS, y: cy, text: '/', color: SLASH },
          total: { x: sx + 9 * KILL_CIPHERS + 7, y: cy, text: pad3(total), color: WHITE },
        };
      } else {
        count = {
          digits: [{ x: sx, y: cy, text: pad3(g.kills), color: WHITE }],
          slash: { x: sx + 21, y: cy, text: '/', color: SLASH },
          total: { x: sx + 28, y: cy, text: pad3(total), color: WHITE },
        };
      }
    }
    const logo = env.logoOf(g.guildId);
    rows.push({
      index: j,
      own,
      guildId: g.guildId,
      war: g.war,
      kills: g.kills,
      rect,
      fill,
      rank: { x: cx, y: cy, text: `${String(j + 1)}.`, color: WHITE },
      logo: logo > 0 ? { logo, x: logoAt.x, y: logoAt.y, scale } : null,
      name: { x: cx + nameDx, y: cy, text: cutOver(guildName, 14), color: NAME },
      sep: fight
        ? { x: cx + sepDx - 3, y: cy, text: 'vs', color: WHITE }
        : { x: cx + sepDx, y: cy, text: '-', color: WHITE },
      state: st ? { x: sx, y: cy, text: st.text, color: st.color } : null,
      count,
      lamp:
        st && st.rgb >= 0
          ? { ...rect, thick: own ? 4 : 2, rgb: st.rgb, halfFrames: st.halfFrames }
          : null,
    });
  });
  return { rows };
}
