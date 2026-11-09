// `WndGuildCombatState` (`CWndGuildCombatState`, `src:_Interface/WndField.cpp:16680-17135`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-49): `APP_GUILDCOMBAT_OFFERSTATE` (547, 304 x 480), the list of
// the guilds that applied for the siege. `STATIC2` counts down to the next opening ("D H M S" from `seconds_to_next`, recomputed every process), `LISTBOX1` has one row "No.%.2d\t\t<guild>" per
// applicant in the order sent, `STATIC5` is the own rank (blank when the guild is not in the list, red above the 15 admitted guilds), `STATIC6` the own bid and `STATIC8` the total of the bids
// (both money style). `BUTTON1` closes the window and asks for the bid window, `BUTTON2` asks for a fresh list (the answer replaces the window), `CLOSE` closes.
// V15 opened the 676 layout (`APP_GUILDCOMBAT_1TO1_OFFERSTATE`, no prize line) in both modes; the plan's 547 layout is used here (it has the prize line), so the title is that of the siege.

import {
  APP_GUILDCOMBAT_OFFERSTATE,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_CLOSE,
  WIDC_LISTBOX1,
  WIDC_STATIC2,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC8,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT_OFFERSTATE,
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
} from '../../../gen/defs/text';
import type { ListBox } from '../../controls/ListBox';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { WSS_MONEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { timeSpan } from './format';
import { MAX_JOIN_GUILD_SIZE, type SiegeHost, type StateRequest } from './ports';
import { guildRow, RANK_OUT_COLOR } from './rows';

export interface StateDeps {
  readonly host: SiegeHost;
}

export class WndGuildCombatState extends WndNeuz {
  // typed `number`: the 1:1 shell (`ui/windows/gc1to1`) is a subclass on `APP_GUILDCOMBAT_1TO1_OFFERSTATE`
  readonly appId: number = APP_GUILDCOMBAT_OFFERSTATE;
  readonly #request: StateRequest;
  readonly #host: SiegeHost;
  #list: ListBox | null = null;
  #time: Static | null = null;
  #rank: Static | null = null;
  /** `m_tEndTime` in the clock of the UI (ms). */
  #endMs = 0;
  /** The countdown text shown last. */
  #shown = '';

  constructor(request: StateRequest, deps: StateDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get list(): ListBox | null {
    return this.#list;
  }
  get timeStatic(): Static | null {
    return this.#time;
  }
  get rankStatic(): Static | null {
    return this.#rank;
  }
  get bidStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC6);
  }
  get prizeStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC8);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    this.setTitle(this.#host.text(TID_GAME_GUILDCOMBAT_OFFERSTATE));
    this.#list = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#time = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#rank = this.getDlgItem<Static>(WIDC_STATIC5);
    // the layout's sample caption ("3 days 12:10:31") never belongs on the screen (FIX: the original showed it while no time was known)
    this.#time?.setTitle('');
    r.guilds.forEach((g, i) => {
      this.#list?.addString(guildRow(i + 1, g));
    });
    this.setRate(r.rank);
    this.setGold(r.myBid);
    this.setTotalGold(r.prize);
    this.#endMs = this.env.host.now() + Math.max(0, r.secondsToNext) * 1000;
    this.#tick(this.env.host.now());
    this.moveParentCenter();
  }

  /** `SetRate`: the own rank, blank for none, red when above the admitted guilds (FIX of the leftover constant 8). */
  setRate(rate: number): void {
    const s = this.#rank;
    if (!s) return;
    s.setTitle(rate === -1 ? ' ' : String(rate));
    if (rate > MAX_JOIN_GUILD_SIZE) s.setTextColor(RANK_OUT_COLOR);
  }

  /** `SetGold`. */
  setGold(penya: number): void {
    const s = this.bidStatic;
    if (!s) return;
    s.addWndStyle(WSS_MONEY);
    s.setTitle(String(penya));
  }

  /** The total of the bids. */
  setTotalGold(penya: number): void {
    const s = this.prizeStatic;
    if (!s) return;
    s.addWndStyle(WSS_MONEY);
    s.setTitle(String(penya));
  }

  /** The text of the countdown for `seconds` left: `"<days> <hours> <minutes> <seconds>"` of the four `TID_PK_LIMIT_*` templates. */
  countdownText(seconds: number): string {
    const t = timeSpan(seconds);
    const h = this.#host;
    return [
      h.fmt(TID_PK_LIMIT_DAY, t.days),
      h.fmt(TID_PK_LIMIT_HOUR, t.hours),
      h.fmt(TID_PK_LIMIT_MINUTE, t.minutes),
      h.fmt(TID_PK_LIMIT_SECOND, t.seconds),
    ].join(' ');
  }

  #tick(now: number): void {
    const left = Math.floor((this.#endMs - now) / 1000);
    // only while time is left (`if( m_tCurrentTime > 0 )`)
    if (left <= 0) return;
    const text = this.countdownText(left);
    if (text === this.#shown) return;
    this.#shown = text;
    this.#time?.setTitle(text);
  }

  override process(now: number): void {
    this.#tick(now);
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_CLOSE) {
        this.destroy();
        return true;
      }
      if (id === WIDC_BUTTON1) {
        // `Destroy(); SendGuildCombatWindow()`
        this.destroy();
        this.#request.apply();
        return true;
      }
      if (id === WIDC_BUTTON2) {
        this.#request.refresh();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
