// `WndSecretRoomOfferState` (`CWndSecretRoomOfferState`, `src:_Interface/WndSecretRoom.cpp:855-1015`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-56): `APP_SECRETROOM_OFFERSTATE` (704, 304 x 480), the
// list of the guilds that bid for the secret room. `STATIC2` counts down to the next opening ("D H M S" from `remain_s`, recomputed in `process` ONLY while time is left, KEEP: at 0 the text
// stays at its last value), `LISTBOX1` has one row "No.%.2d\t\t<guild>" per KNOWN bidder in rank order, `STATIC5` is the own rank (blank for 0: FIX, the original printed "0"; red above the
// eight admitted guilds), `STATIC6` the own bid (money style). `BUTTON1` closes the window and asks for the bid window, `BUTTON2` asks for a fresh list (the answer replaces the window), CLOSE
// closes. The layout's sample caption is cleared (the twin `WndGuildCombatState` does the same; the spec asked to keep it for `remain_s = 0`: the sample text never belongs on the screen).

import {
  APP_SECRETROOM_OFFERSTATE,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_CLOSE,
  WIDC_LISTBOX1,
  WIDC_STATIC2,
  WIDC_STATIC5,
  WIDC_STATIC6,
} from '../../../gen/defs/resdata';
import type { ListBox } from '../../controls/ListBox';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { WSS_MONEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { OfferStateRequest, SecretRoomHost } from './ports';
import { countdownText, guildRow, RANK_OUT_COLOR, SR_RANK_RED_ABOVE } from './rows';

export interface OfferStateDeps {
  readonly host: SecretRoomHost;
}

export class WndSecretRoomOfferState extends WndNeuz {
  readonly appId = APP_SECRETROOM_OFFERSTATE;
  readonly #request: OfferStateRequest;
  readonly #host: SecretRoomHost;
  #list: ListBox | null = null;
  #time: Static | null = null;
  #rank: Static | null = null;
  /** `m_tEndTime` in the clock of the UI (ms). */
  #endMs = 0;
  /** The countdown text shown last. */
  #shown = '';

  constructor(request: OfferStateRequest, deps: OfferStateDeps) {
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
  get tenderStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC6);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    this.#list = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#time = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#rank = this.getDlgItem<Static>(WIDC_STATIC5);
    this.#time?.setTitle('');
    r.guilds.forEach((g, i) => {
      this.#list?.addString(guildRow(i + 1, g));
    });
    this.setRank(r.rank);
    this.setTender(r.myTender);
    this.#endMs = this.env.host.now() + Math.max(0, r.secondsToNext) * 1000;
    this.#tick(this.env.host.now());
    this.moveParentCenter();
  }

  /** The own rank: blank for 0 (FIX), red above the admitted guilds. */
  setRank(rank: number): void {
    const s = this.#rank;
    if (!s) return;
    s.setTitle(rank <= 0 ? ' ' : String(rank));
    if (rank > SR_RANK_RED_ABOVE) s.setTextColor(RANK_OUT_COLOR);
  }

  /** The own bid. */
  setTender(penya: number): void {
    const s = this.tenderStatic;
    if (!s) return;
    s.addWndStyle(WSS_MONEY);
    s.setTitle(String(penya));
  }

  #tick(now: number): void {
    const left = Math.floor((this.#endMs - now) / 1000);
    // only while time is left (`if( m_tCurrentTime > 0 )`)
    if (left <= 0) return;
    const text = countdownText(this.#host, left);
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
        // `Destroy(); SendSecretRoomTenderOpenWnd()`
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
