// The "busy checks" of the NPC and player menu actions (Plan/45 §11.2): an action is refused with a chat text while one of these windows is
// open: the bank (`TID_GAME_BANKTRADESELF`), the post (`TID_MAIL_OTHERWORK`), the bank confirm, the guild bank and the bank password
// (`TID_GAME_BANKSTILLUSING`), the face shop (`TID_GAME_FACESHOPUSING`), the hair shop (`TID_GAME_BEAUTYSHOPUSING`), the summon angel window
// (`TID_GAME_SUMMONANGELUSING`) and a trade or a trade request (`TID_GAME_YETTRADE`). Shared by the shop and the beauty shops.

import {
  APP_BANK_PASSWORD,
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_COMMON_BANK,
  APP_CONFIRM_BANK,
  APP_CONFIRM_TRADE,
  APP_GUILD_BANK,
  APP_POST,
  APP_SUMMON_ANGEL,
  APP_TRADE,
} from '../../gen/defs/resdata';
import {
  TID_GAME_BANKSTILLUSING,
  TID_GAME_BANKTRADESELF,
  TID_GAME_BEAUTYSHOPUSING,
  TID_GAME_FACESHOPUSING,
  TID_GAME_SUMMONANGELUSING,
  TID_GAME_YETTRADE,
  TID_MAIL_OTHERWORK,
} from '../../gen/defs/text';

/** The windows that make a menu action refuse, with the text of the refusal, in the order of the original. */
export const BUSY_WINDOWS: readonly (readonly [app: number, tid: number])[] = [
  [APP_COMMON_BANK, TID_GAME_BANKTRADESELF],
  [APP_POST, TID_MAIL_OTHERWORK],
  [APP_CONFIRM_BANK, TID_GAME_BANKSTILLUSING],
  [APP_GUILD_BANK, TID_GAME_BANKSTILLUSING],
  [APP_BANK_PASSWORD, TID_GAME_BANKSTILLUSING],
  [APP_BEAUTY_SHOP_SKIN, TID_GAME_FACESHOPUSING],
  [APP_BEAUTY_SHOP_EX, TID_GAME_BEAUTYSHOPUSING],
  [APP_SUMMON_ANGEL, TID_GAME_SUMMONANGELUSING],
  [APP_CONFIRM_TRADE, TID_GAME_YETTRADE],
  [APP_TRADE, TID_GAME_YETTRADE],
];

/** The `TID` of the first open busy window, `null` when none is open. */
export function busyText(isOpen: (app: number) => boolean): number | null {
  for (const [app, tid] of BUSY_WINDOWS) if (isOpen(app)) return tid;
  return null;
}
