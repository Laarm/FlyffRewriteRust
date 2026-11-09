// The kill log of the siege (Plan/45 §28.4; `src:Neuz/DPClient.cpp:7480-7660`): the lines of the result window's log tab (`GC_LOG`) and the real-time chat line (`GC_LOG_REALTIME`).
//
// Per kill two lines and a blank one:
//   "[<guildAtk>] <master> <attacker>(<n> points) Attack → [<guildDef>] <master|defender> <victim>"        coloured `#cff009c00` when I or my guild attacked, the victim part `#cff9c0000` when my guild died
//   "< Basic Point +2[, Guild Master Bonus +1][, Defender Bonus +1][, Kill bonus +1][, Resurrecting Bonus +1] >"
// The master marks come from the guilds' masters (`GuildBrief.id_master`); on the victim side the defender wins over the master. A vanished guild is "NotGuild" (FIX: the original dereferenced
// NULL).

import {
  TID_GAME_ATTACK,
  TID_GAME_GC_LOG1,
  TID_GAME_GC_LOG2,
  TID_GAME_GC_LOG3,
  TID_GAME_GC_LOG4,
  TID_GAME_GC_LOG5,
  TID_GAME_GC_LOG_DEFENDER,
  TID_GAME_GC_LOG_GUILD,
  TID_GAME_GC_LOG_MASTER,
  TID_GAME_GC_LOG_POINT,
} from '../../../gen/defs/text';
import { fmt } from '../../../ui/windows/siege/format';

/** The name of a guild that is not known any more. */
export const NOT_GUILD = 'NotGuild';

/** One kill (`GCGETPOINT`). */
export interface LogEntry {
  readonly guildAttack: number;
  readonly guildDefence: number;
  readonly playerAttack: number;
  readonly playerDefence: number;
  readonly point: number;
  readonly killOtherGuild: boolean;
  readonly master: boolean;
  readonly defender: boolean;
  readonly lastLife: boolean;
}

export interface LogEnv {
  readonly selfId: number;
  /** The own guild, 0 for none. */
  readonly selfGuild: number;
  /** The name of a guild; `null` when it is unknown. */
  guildName(id: number): string | null;
  /** The master of a guild (`GuildBrief.id_master`), 0 when unknown. */
  masterOf(guildId: number): number;
  /** The name of a character, `''` when unknown. */
  nameOf(id: number): string;
  /** `prj.GetText( tid )`. */
  text(tid: number): string;
}

const guildNameOf = (env: LogEnv, id: number): string => env.guildName(id) ?? NOT_GUILD;

/** The two lines of one kill, without the colour of the blank line (`line1\nline2\n\n` joins them). */
export function killLines(e: LogEntry, env: LogEnv): { head: string; bonus: string } {
  const atkMaster = env.masterOf(e.guildAttack) === e.playerAttack && e.playerAttack !== 0;
  const defMaster = env.masterOf(e.guildDefence) === e.playerDefence && e.playerDefence !== 0;
  const master = env.text(TID_GAME_GC_LOG_MASTER);
  const mark1 = atkMaster ? master : '';
  // the defender mark wins over the master mark on the victim side
  const mark2 = e.defender ? env.text(TID_GAME_GC_LOG_DEFENDER) : defMaster ? master : '';
  const atk = fmt(env.text(TID_GAME_GC_LOG_GUILD), [guildNameOf(env, e.guildAttack)]);
  const point = fmt(env.text(TID_GAME_GC_LOG_POINT), [e.point]);
  const def = fmt(env.text(TID_GAME_GC_LOG_GUILD), [guildNameOf(env, e.guildDefence)]);
  const attack = env.text(TID_GAME_ATTACK);
  const attacker = env.nameOf(e.playerAttack);
  const victim = env.nameOf(e.playerDefence);
  const left = `${atk} ${mark1} ${attacker}${point}`;
  const right = `${def} ${mark2} ${victim}`;
  let head: string;
  if (e.playerAttack === env.selfId || (env.selfGuild !== 0 && env.selfGuild === e.guildAttack)) {
    head = `#cff009c00${left}#nc ${attack} → ${right}`;
  } else if (env.selfGuild !== 0 && env.selfGuild === e.guildDefence) {
    head = `${left} ${attack} → #cff9c0000${right}#nc`;
  } else {
    head = `${left} ${attack} → ${right}`;
  }
  let bonus = env.text(TID_GAME_GC_LOG1);
  if (e.master) bonus += `, ${env.text(TID_GAME_GC_LOG2)}`;
  if (e.defender) bonus += `, ${env.text(TID_GAME_GC_LOG3)}`;
  if (e.killOtherGuild) bonus += `, ${env.text(TID_GAME_GC_LOG4)}`;
  if (e.lastLife) bonus += `, ${env.text(TID_GAME_GC_LOG5)}`;
  return { head, bonus: `< ${bonus} >` };
}

/** The whole log text in markup: per kill the head, the bonus line and a blank line (`InsertLog`, `"\r\n"`). */
export function logText(entries: readonly LogEntry[], env: LogEnv): string {
  return entries
    .map((e) => {
      const { head, bonus } = killLines(e, env);
      return `${head}\n${bonus}\n\n`;
    })
    .join('');
}

/** `GuildLog_YYYYMMDD.txt` (the local date, `CTime::GetCurrentTime`). */
export function logFileName(date: Date): string {
  const y = String(date.getFullYear());
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `GuildLog_${y}${m}${d}.txt`;
}

/** The chat line of `GC_LOG_REALTIME` (the English branch of the original is the one kept). */
export function realtimeLine(
  guildAtk: string,
  attacker: string,
  guildDef: string,
  defender: string,
  point: number,
): string {
  return `[${guildAtk}][${attacker}] has killed [${guildDef}][${defender}] and has gained ${String(point)} points.`;
}
