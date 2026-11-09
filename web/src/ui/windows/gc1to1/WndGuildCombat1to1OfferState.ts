// `WndGuildCombat1to1OfferState` (`CWndGuildCombatState( 1 )` on `APP_GUILDCOMBAT_1TO1_OFFERSTATE`, `src:_Interface/WndField.cpp:16923-17150`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.5; WP-UI-55): the
// application status of the 1:1 guild combat (676, 304 x 480, the layout V15 really opened in both modes; the siege opens 547). A subclass of the siege's state window: the title is
// `TID_GAME_GUILDCOMBAT_1TO1_OFFERSTATE`, the layout has no prize line (`STATIC8`: `prizeStatic` is `null`, `StateRequest.prize` is ignored), and the own rank turns red above 8 guilds
// (`if( 8 < nRate )`; `MaxJoinGuild` 8 of `GuildCombat1TO1.lua`: the original constant is right in this mode, unlike the siege's FIX to 15). Everything else is the base: the rows
// `"No.%.2d\t\t<guild>"`, the countdown of `secondsToNext`, `BUTTON1` closes and asks for the tender window (`apply()`), `BUTTON2` asks a fresh list (`refresh()`), `CLOSE` closes.
//
// There is NO opener in the lord task: the 1:1 feature (`features/events/gc1to1`, task ui-minigames) calls `ui.gc1to1.openOfferState( request )` from `Gc1to1TenderViewNtf` behind the
// flag `events.guild_combat_1to1` (22).

import { APP_GUILDCOMBAT_1TO1_OFFERSTATE } from '../../../gen/defs/resdata';
import { TID_GAME_GUILDCOMBAT_1TO1_OFFERSTATE } from '../../../gen/defs/text';
import type { SiegeHost, StateRequest } from '../siege/ports';
import { RANK_OUT_COLOR } from '../siege/rows';
import { WndGuildCombatState, type StateDeps } from '../siege/WndGuildCombatState';
import { MAX_JOIN_GUILD_1TO1 } from './rows';

export class WndGuildCombat1to1OfferState extends WndGuildCombatState {
  override readonly appId: number = APP_GUILDCOMBAT_1TO1_OFFERSTATE;
  readonly #host: SiegeHost;

  constructor(request: StateRequest, deps: StateDeps) {
    super(request, deps);
    this.#host = deps.host;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `PaintFrame` of the 1:1 mode
    this.setTitle(this.#host.text(TID_GAME_GUILDCOMBAT_1TO1_OFFERSTATE));
  }

  /** `SetRate`: the own rank, blank for none, red above the 8 admitted guilds. */
  override setRate(rate: number): void {
    const s = this.rankStatic;
    if (!s) return;
    s.setTitle(rate === -1 ? ' ' : String(rate));
    if (rate > MAX_JOIN_GUILD_1TO1) s.setTextColor(RANK_OUT_COLOR);
  }
}
