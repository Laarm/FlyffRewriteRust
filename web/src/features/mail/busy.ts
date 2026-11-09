// The "busy checks" of the post menu action (`CWndWorld::OnCommand(MMI_POST)`, `src:_Interface/WndWorld.cpp:4713-4800`; Plan/45 §11.2): in the order of the
// original, the first open window decides, with the chat text of the refusal.

import {
  APP_BANK_PASSWORD,
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_COMMON_BANK,
  APP_CONFIRM_BANK,
  APP_GUILD_BANK,
  APP_POST,
  APP_SHOPEX,
  APP_SUMMON_ANGEL,
  APP_TRADE,
} from '../../gen/defs/resdata';
import {
  TID_GAME_BANKSTILLUSING,
  TID_GAME_BANKTRADESELF,
  TID_GAME_BEAUTYSHOPUSING,
  TID_GAME_FACESHOPUSING,
  TID_GAME_SUMMONANGELUSING,
  TID_GAME_TRADENOTBEAUTYSHOP,
  TID_GAME_TRADENOTPOST,
  TID_MAIL_OTHERWORK,
} from '../../gen/defs/text';

/** `APP_REPAIR` never exists in this client (Plan/45 §17). */
export const POST_BUSY: readonly (readonly [app: number, tid: number])[] = [
  [APP_TRADE, TID_GAME_TRADENOTPOST],
  [APP_POST, TID_MAIL_OTHERWORK],
  [APP_COMMON_BANK, TID_GAME_BANKTRADESELF],
  [APP_CONFIRM_BANK, TID_GAME_BANKSTILLUSING],
  [APP_GUILD_BANK, TID_GAME_BANKSTILLUSING],
  [APP_BANK_PASSWORD, TID_GAME_BANKSTILLUSING],
  [APP_SHOPEX, TID_GAME_TRADENOTBEAUTYSHOP],
  [APP_BEAUTY_SHOP_SKIN, TID_GAME_FACESHOPUSING],
  [APP_BEAUTY_SHOP_EX, TID_GAME_BEAUTYSHOPUSING],
  [APP_SUMMON_ANGEL, TID_GAME_SUMMONANGELUSING],
];

/** The text of the first open window that refuses the post, `null` when none does. */
export function postBusy(isOpen: (app: number) => boolean): number | null {
  for (const [app, tid] of POST_BUSY) if (isOpen(app)) return tid;
  return null;
}
