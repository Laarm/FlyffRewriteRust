// The browser-side chat commands of this task (Plan/45 §8.6; `FuncTextCmd.cpp` rows with side CLIENT): registered with the
// chat feature's local-command registry by their table row (`gen/gm.ts` `GmCommand.row`).
//
//   5 /position (/pos)      the own position, `TID_GAME_NOWPOSITION` with `%f` (6 decimals)
//   8 /Time (/ti)           "Real Time - HH:MM:SS" and "Madrigal Time - h:m:s"
//   27 /ignorelist          the ignore list of the server (`APP_CHATTING_BLOCKING_LIST`, read only: the window of the messenger feature)
//   114 /open, 115 /close   a window by its applet keyword
//
// Rows 13-24 (/tradeagree ... /shoutrefuse) and 25-26 (/ignore, /unignore) are `both` since M6 (WP-GM-22): the browser sends
// them as `ChatReq`, the server stores the setting, answers with the row's `DefinedText` and sends `ChatSettingsNtf`.
//
// Rows 68 (/destroyguild), 93 (/makepetfeed) and 105 (/ClearPetName) have no handler yet (guild window M6, pet windows M4):
// they are sent and the server answers like for an unknown command.

import type { ClientContext } from '../../app/context';
import { gameClockNow } from '../../game/world/environment';
import { TID_GAME_NOWPOSITION } from '../../gen/defs/text';
import { wallNow } from '../../platform/time';
import { localCommandsOf } from '../chat/commands';
import { STYLE_CLIENT_LINE, COLOR_WHITE } from '../chat/display';
import { type FeatureContext, fullUi } from '../common/module';
import { formatTid } from '../common/text';
import { messengerOf } from '../messenger/api';

const pad2 = (n: number): string => String(n).padStart(2, '0');

/** `"Real Time - %H:%M:%S"` of a date (local time). */
export function realTimeLine(d: Date): string {
  return `Real Time - ${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

/** `"Madrigal Time - %d:%d:%d"`: no zero padding. */
export function gameTimeLine(hour: number, min: number, sec: number): string {
  return `Madrigal Time - ${String(hour)}:${String(min)}:${String(sec)}`;
}

/** Registers the commands above in the local-command registry of the context. */
export function registerClientCommands(fc: FeatureContext): void {
  const ctx: ClientContext = fc.app;
  const commands = localCommandsOf(ctx);

  const put = (text: string, color: number): void => {
    fullUi(ctx)?.chat.putString(text, null, color, STYLE_CLIENT_LINE);
  };
  const tid = (id: number, args: readonly (string | number)[] = []): void => {
    put(formatTid(fc.texts, id, args), fc.texts.textColor(id));
  };

  commands.set(5, () => {
    const [x, y, z] = ctx.stores.selfStore.getState().pos;
    tid(TID_GAME_NOWPOSITION, [x.toFixed(6), y.toFixed(6), z.toFixed(6)]);
  });

  commands.set(8, () => {
    put(realTimeLine(new Date(wallNow())), COLOR_WHITE);
    const g = gameClockNow(
      ctx.stores.worldStore.getState(),
      ctx.net.clock.serverUnixMsNow(ctx.now()),
    );
    put(gameTimeLine(g.hour, g.min, g.sec), COLOR_WHITE);
  });

  // `CWndChattingBlockingList`: the list the server keeps (`ChatSettingsNtf`), in a read-only window
  commands.set(27, () => {
    messengerOf(ctx)?.openBlockList();
  });

  commands.set(114, ({ args }) => {
    const ui = fullUi(ctx);
    const entry = ui?.applets.byKeyword(args[0] ?? '');
    if (ui && entry) ui.wm.openApplet(entry.app);
  });
  commands.set(115, ({ args }) => {
    const ui = fullUi(ctx);
    const entry = ui?.applets.byKeyword(args[0] ?? '');
    if (ui && entry) ui.wm.close(entry.app);
  });
}
