// `WndGuildCombatOffer` (`CWndGuildCombat1to1Offer( nCombatType )`, `src:_Interface/WndGuildCombat1to1.cpp:545-805`, and the pre-V11 `CWndGuildCombatOffer`, `WndField.cpp:15945-16125`; ref/22 §17 K,
// Plan/42 §19.4 K, 45 §28.4; WP-UI-49): the bid window of the guild siege. ONE class for both layouts: `APP_GUILDCOMBAT_1TO1_OFFER` (675, 304 x 272) is the V15 siege bid window (mode 0, title
// `TID_GAME_GUILDCOMBAT_OFFER`; mode 1 is the 1:1 tender, title `TID_GAME_GUILDCOMBAT_1TO1_OFFER`, WP-CFEAT-45) and `APP_GUILDCOMBAT_OFFER` (545, `classic`) is built for CA1 and has no opener.
//
// The edit (`WIDC_EDIT1`) takes digits only, at most ten of them; the value read from it is clamped to 2 147 483 647 (FIX: the original `atoi` overflowed). ACCEPT checks in the order of the
// original and with the texts of the `1TO1` family in BOTH modes (KEEP: V15 never switched to the plain `GUILDCOMBAT_` texts): a bid that is not higher than the guild's current one, or lower
// than the minimum for a guild without a bid, is refused with a box; any other bid asks "Do you want to apply for N penya more than the M you applied for?" (OK / Cancel) and OK hands the
// amount to the request and closes the window. `WIDC_TEXT1` shows the red-chip information panel; the prize line (`WIDC_STATIC6`, money style) belongs to the 545 layout only.

import {
  APP_GUILDCOMBAT_1TO1_OFFER,
  APP_GUILDCOMBAT_OFFER,
  WIDC_ACCEPT,
  WIDC_CLOSE,
  WIDC_EDIT1,
  WIDC_STATIC6,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT1TO1_LIMIT_MIN,
  TID_GAME_GUILDCOMBAT1TO1_MORE_CURRENT_REQUEST,
  TID_GAME_GUILDCOMBAT1TO1_MORE_REQUEST,
  TID_GAME_GUILDCOMBAT_1TO1_OFFER,
  TID_GAME_GUILDCOMBAT_OFFER,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, WSS_MONEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { I32_MAX, type OfferRequest, type SiegeHost } from './ports';
import { OFFER_MAX_DIGITS, parseBid } from './rows';

export interface OfferDeps {
  readonly host: SiegeHost;
}

export class WndGuildCombatOffer extends WndNeuz {
  readonly appId: number;
  readonly #request: OfferRequest;
  readonly #host: SiegeHost;
  #edit: EditCtrl | null = null;

  constructor(request: OfferRequest, deps: OfferDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.appId = request.classic === true ? APP_GUILDCOMBAT_OFFER : APP_GUILDCOMBAT_1TO1_OFFER;
    this.putRegInfo = false;
  }

  get mode(): 0 | 1 {
    return this.#request.mode;
  }
  get edit(): EditCtrl | null {
    return this.#edit;
  }
  get acceptButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_ACCEPT);
  }
  get closeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CLOSE);
  }
  get infoText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  /** The prize line of the 545 layout. */
  get prizeStatic(): Static | null {
    return this.#request.classic === true ? this.getDlgItem<Static>(WIDC_STATIC6) : null;
  }

  /** `m_dwBackupGold`: the bid the new one is compared with (the minimum for a guild without a bid). */
  get backupGold(): number {
    return this.#request.myBid !== 0 ? this.#request.myBid : this.#request.min;
  }
  /** The amount the edit starts with: the current bid, or the minimum when there is none. */
  get preset(): number {
    return this.backupGold;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    // `CWndGuildCombat1to1Offer::PaintFrame` replaces the title by the one of the mode; the pre-V11 window kept the title of its layout
    if (r.classic !== true) {
      this.setTitle(
        this.#host.text(
          r.mode === 0 ? TID_GAME_GUILDCOMBAT_OFFER : TID_GAME_GUILDCOMBAT_1TO1_OFFER,
        ),
      );
    }
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    if (this.#edit) {
      this.#edit.addWndStyle(EBS_NUMBER);
      this.#edit.setMaxLength(OFFER_MAX_DIGITS);
      this.#edit.setString(String(this.preset));
    }
    // the red-chip panel (`GuildCombatRedChipGuild.inc`)
    if (r.info !== '') this.infoText?.setString(r.info);
    const prize = this.prizeStatic;
    if (prize) {
      prize.addWndStyle(WSS_MONEY);
      prize.setTitle(String(r.prize ?? 0));
    }
    this.enableAccept(r.acceptEnabled ?? true);
    this.moveParentCenter();
  }

  /** `EnableAccept`. */
  enableAccept(on: boolean): void {
    this.acceptButton?.enableWindow(on);
  }

  /** `SetGold`: the text of the edit. */
  setGold(penya: number): void {
    this.#edit?.setString(String(penya));
  }

  /**
   * `OnChildNotify( WIDC_ACCEPT )`: the refusals, then the confirmation. Returns the amount that was put to the confirm box, `null` for a refusal (the tests read it; the original returned
   * `FALSE`).
   */
  accept(): number | null {
    const r = this.#request;
    const typed = this.#edit?.getString() ?? '';
    const value = parseBid(typed);
    // FIX: a number above INT_MAX is shown as the clamped one
    if (this.#edit && Number(typed.replace(/[^0-9]/g, '') || 0) > I32_MAX) {
      this.#edit.setString(String(I32_MAX));
    }
    const backup = this.backupGold;
    if (r.myBid !== 0) {
      if (value <= backup) {
        this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT1TO1_MORE_CURRENT_REQUEST));
        return null;
      }
    } else if (value < r.min) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT1TO1_LIMIT_MIN));
      return null;
    }
    const text =
      r.myBid === 0
        ? this.#host.fmt(TID_GAME_GUILDCOMBAT1TO1_MORE_REQUEST, 0, value)
        : this.#host.fmt(TID_GAME_GUILDCOMBAT1TO1_MORE_REQUEST, backup, value - backup);
    this.#host.confirm(
      text,
      () => {
        r.bid(value);
        this.destroy();
      },
      this,
    );
    return value;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_ACCEPT) {
        this.accept();
        return true;
      }
      if (id === WIDC_CLOSE) {
        this.destroy();
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
