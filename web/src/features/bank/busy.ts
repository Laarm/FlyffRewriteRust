// The "busy checks" of the bank menu action (`CWndWorld::OnCommand(MMI_BANKING)`, `src:_Interface/WndWorld.cpp:5639-5724`; Plan/45 §11.2): in the order of
// the original, the first open window decides. `null` = free; a `tid` = the chat text of the refusal; `'silent'` = the refusal without a text.

import {
  APP_BANK_PASSWORD,
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_COMMON_BANK,
  APP_CONFIRM_BANK,
  APP_DROP_ITEM,
  APP_GUILD_BANK,
  APP_SHOPEX,
  APP_SUMMON_ANGEL,
  APP_TRADE,
} from '../../gen/defs/resdata';
import {
  TID_GAME_BANKNOTRADE,
  TID_GAME_BANKSTILLUSING,
  TID_GAME_BEAUTYSHOPUSING,
  TID_GAME_SUMMONANGELUSING,
  TID_GAME_TRADENOBANK,
} from '../../gen/defs/text';

export type BusyReason = number | 'silent' | null;

/** `APP_REPAIR` never exists in this client (Plan/45 §17); the rest follows the original's order. */
export const BANK_BUSY: readonly (readonly [app: number, tid: number | 'silent'])[] = [
  [APP_TRADE, TID_GAME_TRADENOBANK],
  [APP_SHOPEX, TID_GAME_BANKNOTRADE],
  [APP_COMMON_BANK, TID_GAME_BANKSTILLUSING],
  [APP_CONFIRM_BANK, TID_GAME_BANKSTILLUSING],
  [APP_BANK_PASSWORD, TID_GAME_BANKSTILLUSING],
  [APP_DROP_ITEM, 'silent'],
  [APP_GUILD_BANK, TID_GAME_BANKSTILLUSING],
  [APP_BEAUTY_SHOP_SKIN, TID_GAME_BEAUTYSHOPUSING],
  [APP_BEAUTY_SHOP_EX, TID_GAME_BEAUTYSHOPUSING],
  [APP_SUMMON_ANGEL, TID_GAME_SUMMONANGELUSING],
];

export function bankBusy(isOpen: (app: number) => boolean): BusyReason {
  for (const [app, why] of BANK_BUSY) if (isOpen(app)) return why;
  return null;
}
