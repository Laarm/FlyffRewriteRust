// The progress texts of `SetQuest` (`CDPClient::OnSetQuest`, `DPClient.cpp:8533-8640`; Plan/45 §28.1; WP-CFEAT-42): which chat lines, captions and legacy sends a changed quest record causes. Pure: the
// module hands in the record before and after, the definition and the names, and executes the effects.
//
// Only for a record with a definition (the original dereferenced the null property of an unknown quest):
//   * the record existed and the new state is not 14: the timer word (`& 0x7fff`) changed -> `TID_QUEST_LIMIT_TIME( min )`; min > 0 also the `CCapTime` caption of the same text (approximated by the
//     HUD caption, `WndWorld.cpp:354-430`); min == 0 -> `TID_QUEST_LIMIT_TIMEOUT`, `TID_QUEST_FAILURE` and the legacy `RemoveQuestReq` (KEEP, Plan/45 OQ11: the server expired the quest already
//     and ignores it, TQ-20-02); the patrol flag changed -> `TID_QUEST_PATROL( zone name )`; a kill count changed -> `TID_QUEST_KILL_NPC( mover, count, required )`; the dialog flag changed ->
//     `TID_QUEST_DIALOG( npc name )`;
//   * then, for every record: state 1 -> `TID_EVE_STARTJOB1`, state 14 -> `TID_EVE_ENDJOB1` except `QUEST_CREGUILD` (142), both through `PutDefinedString`.

import type { QuestClientDef } from '../../gen/bundles';
import { QUEST_CREGUILD } from '../../gen/defs/quest';
import {
  TID_EVE_ENDJOB1,
  TID_EVE_STARTJOB1,
  TID_QUEST_DIALOG,
  TID_QUEST_FAILURE,
  TID_QUEST_KILL_NPC,
  TID_QUEST_LIMIT_TIME,
  TID_QUEST_LIMIT_TIMEOUT,
  TID_QUEST_PATROL,
} from '../../gen/defs/text';
import { QS_END, type QuestRecord } from './book';

export type ProgressEffect =
  /** `g_WndMng.PutString( text )`. */
  | { readonly type: 'put'; readonly text: string }
  /** `g_CapTime.SetTime( min )`: the caption of the timed quest. */
  | { readonly type: 'caption'; readonly text: string; readonly minutes: number }
  /** `SendRemoveQuest( id )`: the legacy request after a timeout. */
  | { readonly type: 'removeReq'; readonly questId: number }
  /** `PutDefinedString( tid, "" )`: the text with its own colour. */
  | { readonly type: 'defined'; readonly tid: number };

/** What the texts need of the client. */
export interface ProgressEnv {
  /** The template of a text id with its arguments applied. */
  fmt(tid: number, args: readonly (string | number)[]): string;
  /** `MoverProp.szName`, `''` for an unknown mover. */
  moverName(id: number): string;
  /** `CHARACTER.m_strName` of an NPC key, `''` for an unknown one. */
  npcName(key: string): string;
}

/** The effects of a `SetQuest` that replaced `prev` (`undefined` for a new record) by `next`. */
export function progressEffects(
  prev: QuestRecord | undefined,
  next: QuestRecord,
  def: QuestClientDef | undefined,
  env: ProgressEnv,
): ProgressEffect[] {
  if (!def) return [];
  const out: ProgressEffect[] = [];
  if (prev && next.state !== QS_END) {
    if (prev.timeLeft !== next.timeLeft) {
      const text = env.fmt(TID_QUEST_LIMIT_TIME, [next.timeLeft]);
      out.push({ type: 'put', text });
      if (next.timeLeft > 0) {
        out.push({ type: 'caption', text, minutes: next.timeLeft });
      } else {
        out.push({ type: 'put', text: env.fmt(TID_QUEST_LIMIT_TIMEOUT, []) });
        out.push({ type: 'put', text: env.fmt(TID_QUEST_FAILURE, []) });
        out.push({ type: 'removeReq', questId: next.id });
      }
    }
    if (prev.patrol !== next.patrol) {
      out.push({ type: 'put', text: env.fmt(TID_QUEST_PATROL, [def.end.patrol?.name ?? '']) });
    }
    for (let i = 0; i < 2; i += 1) {
      if (prev.kills[i] === next.kills[i]) continue;
      const kill = def.end.kill[i];
      out.push({
        type: 'put',
        text: env.fmt(TID_QUEST_KILL_NPC, [
          env.moverName(kill?.mover ?? 0),
          next.kills[i] ?? 0,
          kill?.num ?? 0,
        ]),
      });
    }
    if (prev.dialog !== next.dialog) {
      out.push({
        type: 'put',
        text: env.fmt(TID_QUEST_DIALOG, [env.npcName(def.end.dialog?.npc ?? '')]),
      });
    }
  }
  if (next.state === 1) out.push({ type: 'defined', tid: TID_EVE_STARTJOB1 });
  else if (next.state === QS_END && next.id !== QUEST_CREGUILD) {
    out.push({ type: 'defined', tid: TID_EVE_ENDJOB1 });
  }
  return out;
}
