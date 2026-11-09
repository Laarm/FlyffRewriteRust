// Display routing of the chat messages (Plan/45 §8.3; `CDPClient::OnChat/OnShout/OnWhisper/OnPartyChat/OnGuildChat/
// OnReturnSay`, DPClient.cpp): pure functions from a server message and the world around it to the actions the client
// performs: lines for 42's router (`ui.chat.putString`), balloons and emoticons for 41's overlay, the last whisperer.
// Every function returns a list so that a table test can pin text, colour, style and speaker per message.

import { WI_WORLD_GUILDWAR, WI_WORLD_QUIZ } from '../../gen/defs/world';
import {
  TID_GAME_MSGNOTEXIST,
  TID_GAME_MSGNOTTRANSELF,
  TID_GAME_NOTLOGIN,
  TID_GUILD_CHAT,
} from '../../gen/defs/text';
import { CHATSTY } from '../../ui/chat/styles';
import { formatTid } from '../common/text';
import type { TextApi } from '../../data/clientData';
import { matchEmote, type EmotionRow } from './emotes';

export const COLOR_WHITE = 0xffffffff;
/** Whispers (`0xffffff00`). */
export const COLOR_WHISPER = 0xffffff00;
/** Party chat (`0xff99cc00`). */
export const COLOR_PARTY_CHAT = 0xff99cc00;
/** Messenger lines (`[from] text`). */
export const COLOR_MESSENGER = 0xffffff00;
/** `WorldMsg` (`0xffff99cc`). */
export const COLOR_WORLD_MSG = 0xffff99cc;
/** `Text{kind: 1}` (`0xff00ff00`); a `Text{kind: 3}` line has the same colour. */
export const COLOR_TEXT_GENERAL = 0xff00ff00;
/**
 * `Text{kind: 3}` (`TEXT_CHAT`, BFIX1 #006): a green line in the chat box. Kind 1 is a centre notice (the original's routing for one-line
 * replies, of which the centre keeps 6); the list output of GM commands (`/help`, `/item <search>`, `/rates`) uses this kind.
 */
export const TEXT_KIND_CHAT = 3;
/** `CHATSTY` of the lines that the client writes itself (`g_WndMng.PutString` default `CHATSTY_GAME`). */
export const STYLE_CLIENT_LINE = CHATSTY.GAME;
/** The whisper partner is remembered only below this many bytes (`lstrlen < MAX_NAME`). */
export const MAX_NAME = 64;

/** `CMover::m_dwMode` bits read here (23 §2.1). */
export const MODE_TRANSPARENT = 0x00000002;
export const MODE_GCWAR_RENDER_SKIP = 0x00100000;
export const MODE_QUIZ_RENDER_SKIP = 0x00200000;

/** `AUTH_GAMEMASTER` ('L'). */
export const AUTH_GM = 76;

/** What the display rules know about a speaker (the entity table, 40 §6.3). */
export interface SpeakerView {
  readonly objId: number;
  readonly isPlayer: boolean;
  readonly isNpc: boolean;
  readonly name: string;
  readonly authorization: number;
  readonly mode: number;
  /** An open personal shop (`m_vtInfo.IsVendorOpen()`): `visual.vendorTitle !== null`. */
  readonly vendorOpen: boolean;
}

export interface DisplayEnv {
  readonly texts: TextApi;
  readonly emotions: readonly EmotionRow[];
  /** The own character name (`g_pPlayer->GetName()`). */
  readonly selfName: string;
  readonly worldId: number;
  readonly speaker: (objId: number) => SpeakerView | undefined;
  /** `ignoreNpcDialogue` (`__IGNORE_NPC_DIALOGUE`). */
  readonly ignoreNpcDialogue: boolean;
}

/** The speaker flags 42's router needs (`ChatSpeaker`). */
export interface PutSpeaker {
  isPlayer: boolean;
  vendorOpen: boolean;
  invisibleGm: boolean;
  quizSkip: boolean;
  balloonKind?: 0 | 1 | 2 | 3;
  balloonText?: string;
}

export type ChatAction =
  /** `ui.chat.putString(text, objId, color, style, speaker?)`. */
  | {
      readonly type: 'put';
      readonly text: string;
      readonly objId: number | null;
      readonly color: number;
      readonly style: number;
      readonly speaker?: PutSpeaker;
    }
  /** A balloon over a mover that is NOT a line of the chat window (`g_DialogMsg.AddMessage` direct). */
  | {
      readonly type: 'balloon';
      readonly objId: number;
      readonly text: string;
      readonly kind: 0 | 1 | 2 | 3;
    }
  | {
      readonly type: 'emoticon';
      readonly objId: number;
      readonly set: 'chat' | 'dialog';
      readonly index: number;
    }
  /** `g_Neuz.m_szWhisperName`. */
  | { readonly type: 'lastWhisperer'; readonly name: string };

const sys = (
  env: DisplayEnv,
  tid: number,
  args: readonly (string | number)[] = [],
): ChatAction => ({
  type: 'put',
  text: formatTid(env.texts, tid, args),
  objId: null,
  color: env.texts.textColor(tid),
  style: STYLE_CLIENT_LINE,
});

/** The speaker is a GM in guild-war: its chat is never shown (`GCWAR_RENDER_SKIP_MODE` in world 202). */
function gcwarSkipped(env: DisplayEnv, s: SpeakerView | undefined): boolean {
  return (
    s !== undefined && env.worldId === WI_WORLD_GUILDWAR && (s.mode & MODE_GCWAR_RENDER_SKIP) !== 0
  );
}

/** `ChatNtf{text}` with the speaker's object id (`OnChat`). */
export function displayChatNtf(env: DisplayEnv, objId: number, text: string): ChatAction[] {
  const s = env.speaker(objId);
  if (!s) return [];
  if (gcwarSkipped(env, s)) return [];
  if (text.startsWith('/') || text.startsWith('!')) {
    if (s.isNpc && env.ignoreNpcDialogue) return [];
    const m = matchEmote(text, env.emotions);
    if (m === null) return [];
    if (m.kind === 'balloon') return [{ type: 'balloon', objId, text, kind: 0 }];
    return [{ type: 'emoticon', objId, set: m.kind, index: m.index }];
  }
  if (s.isNpc && env.ignoreNpcDialogue) return [];
  return [
    {
      type: 'put',
      text: `${s.name} : ${text}`,
      objId,
      color: COLOR_WHITE,
      style: CHATSTY.GENERAL,
      speaker: {
        isPlayer: s.isPlayer,
        vendorOpen: s.vendorOpen,
        invisibleGm: s.authorization >= AUTH_GM && (s.mode & MODE_TRANSPARENT) !== 0,
        quizSkip: env.worldId === WI_WORLD_QUIZ && (s.mode & MODE_QUIZ_RENDER_SKIP) !== 0,
        balloonKind: 0,
        balloonText: text,
      },
    },
  ];
}

/** `Shout{obj_id, name, text, color}` (`OnShout`): the line has no speaker object, the balloon is a direct request. */
export function displayShout(
  env: DisplayEnv,
  objId: number,
  name: string,
  text: string,
  color: number,
): ChatAction[] {
  const s = env.speaker(objId);
  if (gcwarSkipped(env, s)) return [];
  const out: ChatAction[] = [
    {
      type: 'put',
      text: `[${name}] ${text}`,
      objId: null,
      color: color >>> 0,
      style: CHATSTY.SHOUT,
    },
  ];
  if (s && text !== '') out.push({ type: 'balloon', objId, text, kind: 3 });
  return out;
}

/** `Whisper{from, to, text, id_from, id_to, search}` (`OnWhisper`). */
export function displayWhisper(
  env: DisplayEnv,
  w: { from: string; to: string; text: string; search: number },
): ChatAction[] {
  const mine = w.from === env.selfName;
  if (w.search !== 0) return [sys(env, TID_GAME_NOTLOGIN)];
  const out: ChatAction[] = [
    {
      type: 'put',
      text: mine ? `-> [${w.to}] ${w.text}` : `<- [${w.from}] ${w.text}`,
      objId: null,
      color: COLOR_WHISPER,
      style: CHATSTY.WHISPER,
    },
  ];
  if (!mine && byteLength(w.from) < MAX_NAME) out.unshift({ type: 'lastWhisperer', name: w.from });
  return out;
}

/** `PartyChat{obj_id, name, text}` (`OnPartyChat`). */
export function displayPartyChat(
  env: DisplayEnv,
  objId: number,
  name: string,
  text: string,
): ChatAction[] {
  const s = env.speaker(objId);
  const out: ChatAction[] = [
    {
      type: 'put',
      text: `[${name}] ${text}`,
      objId: null,
      color: COLOR_PARTY_CHAT,
      style: CHATSTY.PARTY,
    },
  ];
  if (s && text !== '') out.push({ type: 'balloon', objId, text, kind: 1 });
  return out;
}

/** `GuildChat{obj_id, name, text}` (`OnGuildChat`). */
export function displayGuildChat(
  env: DisplayEnv,
  objId: number,
  name: string,
  text: string,
): ChatAction[] {
  const s = env.speaker(objId);
  const out: ChatAction[] = [
    {
      type: 'put',
      text: `[${name}] ${text}`,
      objId: null,
      color: env.texts.textColor(TID_GUILD_CHAT),
      style: CHATSTY.GUILD,
    },
  ];
  if (s && text !== '') out.push({ type: 'balloon', objId, text, kind: 2 });
  return out;
}

/** `ReturnSay{flag, text}` (`OnReturnSay`; the object is the own player). */
export function displayReturnSay(
  env: DisplayEnv,
  moverName: string,
  flag: number,
  text: string,
): ChatAction[] {
  const color = env.texts.textColor(TID_GAME_MSGNOTEXIST);
  let line: string;
  if (flag === 1) line = `[${moverName}] ${text}`;
  else if (flag === 2) line = env.texts.text(TID_GAME_MSGNOTTRANSELF);
  else line = formatTid(env.texts, TID_GAME_MSGNOTEXIST, [text]);
  return [{ type: 'put', text: line, objId: null, color, style: STYLE_CLIENT_LINE }];
}

const byteLength = (s: string): number => new TextEncoder().encode(s).length;
