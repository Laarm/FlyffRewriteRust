// The HUD of the 1:1 guild combat as data (Plan/45 §28.5; `CWndWorld::DrawGuildCombat1to1PlayerInfo`, `DrawGuildCombat1ot1GuildInfo`, `src:_Interface/WndWorld.cpp:2187-2400`): the board of the own
// line-up at the left edge and the board of the round wins at the right edge, drawn for a participant of the arena (`IsPossibleMover( g_pPlayer )`) who has a guild. The timer line is the
// siege HUD's (`SiegeHud`). Pure: `gc1to1HudModel( slice, env )` gives rectangles and texts in the virtual pixels of the UI; `Gc1to1Hud.tsx` only places them.
//
//   left board    rows from (16, 150); the rect from (11, 120), 200 wide, (line-up + 2) x 18 high, ARGB( 60, 192, 217, 217 ); the own guild's name (16 characters + "...") at y - 23 in 0xFFEBAD18;
//                 one row per line-up id that is a member of the own guild, 18 px apart: "%2d." (the number counts EVERY line-up id) at x in 0xFF7093DB, the name (10 characters + "...") at x + 25 in
//                 0xFFFFFF63 - grey 0xFFA8A8A8 when the member's mover is not in view or not a participant - and the state at x + 120: READY 0xFFC891C8, WAR 0xFFFF7F00, WIN 0xFF9370DB, LOSE 0xFFFF0000,
//                 DRAW 0xFF69DB93, anything else "...." white
//   right board   at (width - 150, 200): the rect from (-10, -10), 150 x 100, the same colour; the title WINCOUNT at x + 22 in 0xFFADEAEA; the own guild at y + 30 and the opponent (when known) at y + 60, both
//                 0xFFEBAD18 cut like the left board's, with the round wins at x + 120 in 0xFFF5CCB0

import {
  TID_GAME_GUILDCOMBAT_1TO1_DRAW,
  TID_GAME_GUILDCOMBAT_1TO1_LOSE,
  TID_GAME_GUILDCOMBAT_1TO1_READY,
  TID_GAME_GUILDCOMBAT_1TO1_WAR,
  TID_GAME_GUILDCOMBAT_1TO1_WIN,
  TID_GAME_GUILDCOMBAT_1TO1_WINCOUNT,
} from '../../../gen/defs/text';
import { cutName, padLeft } from '../../../ui/windows/siege/format';
import type { HudPanel, HudText } from '../siege/hudModel';
import { GC1TO1_PLAYER, type Gc1to1Slice } from './slice';

export interface Gc1to1HudModel {
  readonly players: HudPanel;
  readonly wins: HudPanel;
}

export interface Gc1to1HudEnv {
  /** The virtual viewport width (`GetClientRect().Width()` of the world window). */
  readonly width: number;
  /** The own guild's name, `''` without a guild (nothing is drawn then). */
  readonly selfGuildName: string;
  /** `pGuild->GetMember( id )`: the id is a member of the own guild. */
  isMember(id: number): boolean;
  /** `CPlayerDataCenter::GetPlayerString`: the name of a character, `''` while unknown. */
  nameOf(id: number): string;
  /** `IsPossibleMover( pMember->GetMover() )`: the member is in view and takes part. */
  possible(id: number): boolean;
  /** The name of a guild the client knows, `null` for none. */
  guildName(id: number): string | null;
  /** `prj.GetText( tid )`. */
  text(tid: number): string;
}

const BOARD = 0x3cc0d9d9;
const SHADOW = 0xff000000;
const GUILD = 0xffebad18;
const NUMBER = 0xff7093db;
const NAME = 0xffffff63;
const NAME_GREY = 0xffa8a8a8;
const TITLE = 0xffadeaea;
const COUNT = 0xfff5ccb0;
const GAP = 18;

const STATES: Readonly<Record<number, { readonly tid: number; readonly color: number }>> = {
  [GC1TO1_PLAYER.Ready]: { tid: TID_GAME_GUILDCOMBAT_1TO1_READY, color: 0xffc891c8 },
  [GC1TO1_PLAYER.War]: { tid: TID_GAME_GUILDCOMBAT_1TO1_WAR, color: 0xffff7f00 },
  [GC1TO1_PLAYER.Win]: { tid: TID_GAME_GUILDCOMBAT_1TO1_WIN, color: 0xff9370db },
  [GC1TO1_PLAYER.Lose]: { tid: TID_GAME_GUILDCOMBAT_1TO1_LOSE, color: 0xffff0000 },
  [GC1TO1_PLAYER.Draw]: { tid: TID_GAME_GUILDCOMBAT_1TO1_DRAW, color: 0xff69db93 },
};

const text = (x: number, y: number, t: string, color: number): HudText => ({
  x,
  y,
  text: t,
  color,
  shadow: SHADOW,
});

/** The boards for a participant; `null` without a guild (the original dereferenced it). */
export function gc1to1HudModel(slice: Gc1to1Slice, env: Gc1to1HudEnv): Gc1to1HudModel | null {
  if (env.selfGuildName === '') return null;
  // ---- the left board
  const x = 16;
  let y = 150;
  const texts: HudText[] = [text(x, y - (GAP + 5), cutName(env.selfGuildName, 16), GUILD)];
  slice.lineup.forEach((id, i) => {
    if (!env.isMember(id)) return;
    const st = STATES[slice.results.get(id) ?? GC1TO1_PLAYER.Ready];
    const name = cutName(env.nameOf(id), 10);
    texts.push(
      text(x, y, `${padLeft(i + 1, 2)}.`, NUMBER),
      text(x + 25, y, name, env.possible(id) ? NAME : NAME_GREY),
      text(x + 120, y, st ? env.text(st.tid) : '....', st ? st.color : 0xffffffff),
    );
    y += GAP;
  });
  const players: HudPanel = {
    rect: { x: x - 5, y: 150 - 30, w: 200, h: (slice.lineup.length + 2) * GAP, color: BOARD },
    texts,
  };
  // ---- the right board
  const bx = env.width - 150;
  const by = 200;
  const wtexts: HudText[] = [
    text(bx + 22, by, env.text(TID_GAME_GUILDCOMBAT_1TO1_WINCOUNT), TITLE),
    text(bx, by + 30, cutName(env.selfGuildName, 16), GUILD),
    text(bx + 120, by + 30, String(slice.wins[0]), COUNT),
  ];
  const vs = slice.opponentGuild !== 0 ? env.guildName(slice.opponentGuild) : null;
  if (vs !== null) {
    wtexts.push(
      text(bx, by + 60, cutName(vs, 16), GUILD),
      text(bx + 120, by + 60, String(slice.wins[1]), COUNT),
    );
  }
  return {
    players,
    wins: { rect: { x: bx - 10, y: by - 10, w: 150, h: 100, color: BOARD }, texts: wtexts },
  };
}
