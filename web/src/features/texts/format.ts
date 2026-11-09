// Server texts (Plan/45 §7.11, §8.3, §7.12; `CDPClient::OnDefinedText`, `OnText`, `OnSystem`, `OnCaption`, `OnWorldMsg`, `OnBlock`,
// `OnEventMessage`, `OnGameJoin`): pure mappings from a server message to the actions the client performs. A line is a
// `ui.chat.putString` call, a caption goes to the HUD (`features/hud`), a box to the message-box service.

import type { TextApi } from '../../data/clientData';
import {
  TID_ADMIN_ANNOUNCE,
  TID_GAME_LACKSPACE,
  TID_GAME_MSGCHATDENY,
  TID_GAME_MSGDENY,
  TID_GAME_REPAIR_NOTTRADE,
  TID_GAME_TOOLTIP_PLAYTIME,
  TID_GAME_TOOLTIP_PRELEVEL,
  TID_GAME_TOOLTIP_ROLEQUEST,
  TID_GAME_TRADEDENY,
  TID_GAME_TRADELIMITPC,
  TID_GAME_YETFRIEND,
  TID_GAME_YETPARTY,
  TID_GAME_YETTRADE,
} from '../../gen/defs/text';
import { CHATSTY } from '../../ui/chat/styles';
import { formatTid } from '../common/text';
import {
  COLOR_TEXT_GENERAL,
  COLOR_WORLD_MSG,
  STYLE_CLIENT_LINE,
  TEXT_KIND_CHAT,
  type ChatAction,
} from '../chat/display';

export const DISPLAY = {
  UNSPECIFIED: 0,
  MESSAGE: 1,
  CHAT: 2,
  CAPTION: 3,
  CAPTION_SMALL: 4,
} as const;

/** `OnSystem` also puts the notice on the screen as a caption for every language but German and French (the ENG client does). */
export const SYSTEM_NOTICE_CAPTION = true as boolean;

/** `0xff0000` of the legacy event line: opaque red (the alpha byte of the original is never used). */
export const COLOR_EVENT_RED = 0xffff0000;

export type TextAction =
  | ChatAction
  /** `g_Caption1.AddCaption`; `log` copies it into the system chat log window. */
  | {
      readonly type: 'caption';
      readonly text: string;
      readonly small: boolean;
      readonly log: boolean;
    }
  /** `OpenMessageBoxUpper`. */
  | { readonly type: 'box'; readonly text: string };

const line = (text: string, color: number, style: number): TextAction => ({
  type: 'put',
  text,
  objId: null,
  color,
  style,
});

/** The client-written line of a TID: its text with the arguments, in the colour of the text, default style. */
export function tidLine(
  texts: TextApi,
  tid: number,
  args: readonly (string | number)[] = [],
): TextAction {
  return line(formatTid(texts, tid, args), texts.textColor(tid), STYLE_CLIENT_LINE);
}

/** `DefinedText{tid, args, display}`. `CHAT` is ignored (KEEP-QUIRK: the V15 client ignores `CHATTEXT`, ref/24 §7.7). */
export function definedTextActions(
  texts: TextApi,
  tid: number,
  args: readonly string[],
  display: number,
): TextAction[] {
  switch (display) {
    case DISPLAY.CHAT:
      return [];
    case DISPLAY.CAPTION:
    case DISPLAY.CAPTION_SMALL:
      return [
        {
          type: 'caption',
          text: formatTid(texts, tid, args),
          small: display === DISPLAY.CAPTION_SMALL,
          log: true,
        },
      ];
    default:
      return [tidLine(texts, tid, args)];
  }
}

/**
 * `Text{kind, text}`: 1 a green line (a centre notice with the default filters), 2 the upper message box, 3 a green line in the chat box
 * (`TEXT_CHAT`: the list output of GM commands, BFIX1 #006); anything else is ignored.
 */
export function textActions(kind: number, text: string): TextAction[] {
  if (kind === 1) return [line(text, COLOR_TEXT_GENERAL, STYLE_CLIENT_LINE)];
  if (kind === TEXT_KIND_CHAT) return [line(text, COLOR_TEXT_GENERAL, CHATSTY.HELP)];
  if (kind === 2) return [{ type: 'box', text }];
  return [];
}

/** `SystemNotice{text}`: `TID_ADMIN_ANNOUNCE` with the text, system style, and a caption. */
export function systemNoticeActions(texts: TextApi, text: string): TextAction[] {
  const out: TextAction[] = [
    line(
      formatTid(texts, TID_ADMIN_ANNOUNCE, [text]),
      texts.textColor(TID_ADMIN_ANNOUNCE),
      CHATSTY.SYSTEM,
    ),
  ];
  if (SYSTEM_NOTICE_CAPTION) {
    out.push({
      type: 'caption',
      text: formatTid(texts, TID_ADMIN_ANNOUNCE, [text]),
      small: false,
      log: false,
    });
  }
  return out;
}

/** `Caption{small, world_id, text}`: shown when `world_id` is 0 or the current world. */
export function captionActions(
  small: boolean,
  worldId: number,
  currentWorld: number,
  text: string,
): TextAction[] {
  if (worldId !== 0 && worldId !== currentWorld) return [];
  return [{ type: 'caption', text, small, log: true }];
}

/** `WorldMsg{text}`: a pink game line. */
export function worldMsgActions(text: string): TextAction[] {
  return [line(text, COLOR_WORLD_MSG, CHATSTY.GAME)];
}

/** `BlockNtf{gu, name}`: why a request was refused (`CDPClient::OnBlock`). `closeIm` asks to close the IM window of `name`. */
export function blockActions(
  texts: TextApi,
  gu: number,
  name: string,
): { actions: TextAction[]; closeIm: boolean } {
  const withName = (tid: number, colorTid = tid): TextAction =>
    line(formatTid(texts, tid, [name]), texts.textColor(colorTid), STYLE_CLIENT_LINE);
  switch (gu) {
    case 1:
      return { actions: [withName(TID_GAME_MSGCHATDENY)], closeIm: true };
    case 2:
      return { actions: [withName(TID_GAME_MSGDENY)], closeIm: false };
    case 3:
      return { actions: [withName(TID_GAME_TRADEDENY)], closeIm: false };
    case 4:
      // the legacy line uses the colour of TID_GAME_TRADEDENY
      return { actions: [withName(TID_GAME_REPAIR_NOTTRADE, TID_GAME_TRADEDENY)], closeIm: false };
    case 5:
      return { actions: [tidLine(texts, TID_GAME_YETTRADE)], closeIm: false };
    case 6:
      return { actions: [tidLine(texts, TID_GAME_YETFRIEND)], closeIm: false };
    case 7:
      return { actions: [tidLine(texts, TID_GAME_YETPARTY)], closeIm: false };
    case 8:
      return { actions: [tidLine(texts, TID_GAME_TRADELIMITPC)], closeIm: false };
    default:
      return { actions: [], closeIm: false };
  }
}

/** `EventMessage{text, flag, item_id}`; the effects over the mover (`XI_EVE_EVENT_*`) are the sfx pass's (M7, no server sender). */
export function eventMessageActions(
  texts: TextApi,
  flag: number,
  text: string,
  speakerObjId: number,
  isSelf: boolean,
): TextAction[] {
  switch (flag) {
    case 1:
      return isSelf ? [line(text, COLOR_EVENT_RED, STYLE_CLIENT_LINE)] : [];
    case 2:
      return [
        { type: 'put', text, objId: speakerObjId, color: 0xffffffff, style: STYLE_CLIENT_LINE },
      ];
    case 3:
      return isSelf ? [tidLine(texts, TID_GAME_LACKSPACE)] : [];
    default:
      return [];
  }
}

/**
 * The own player's join lines after the join batch (`OnGameJoin`, `DPClient.cpp:4385-4407`): one `TID_GAME_TOOLTIP_ROLEQUEST( title )` line in red per active quest (`questTitles`, from
 * `questOf( ctx ).joinTitles()`: empty while no quest data is loaded, PL-148) BEFORE the level line.
 */
export function joinGreetingActions(
  texts: TextApi,
  level: number,
  savedPlayTimeS: number,
  welcome: readonly string[],
  questTitles: readonly string[] = [],
): TextAction[] {
  const minutes = Math.trunc(savedPlayTimeS / 60);
  const out: TextAction[] = [
    ...questTitles.map((title) =>
      line(formatTid(texts, TID_GAME_TOOLTIP_ROLEQUEST, [title]), 0xffff0000, STYLE_CLIENT_LINE),
    ),
    line(formatTid(texts, TID_GAME_TOOLTIP_PRELEVEL, [level]), 0xffff0000, STYLE_CLIENT_LINE),
    line(
      formatTid(texts, TID_GAME_TOOLTIP_PLAYTIME, [Math.trunc(minutes / 60), minutes % 60]),
      0xff009900,
      STYLE_CLIENT_LINE,
    ),
  ];
  for (const w of welcome) out.push(line(w, 0xffffff00, CHATSTY.HELP));
  return out;
}
