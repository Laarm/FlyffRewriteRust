// `WndSecretRoomOffer` (`CWndSecretRoomOffer`, `src:_Interface/WndSecretRoom.cpp:407-540`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-56): `APP_SECRETROOM_OFFER` (700, 304 x 272), the bid window of the
// secret room. `WIDC_TEXT1` shows the panel `secretroomdesc`; the edit (`WIDC_EDIT1`) takes digits only, at most ten, and the value read from it is clamped to 2 147 483 647 (FIX: the original
// `atoi` into a DWORD overflowed). ACCEPT checks in the order of the original with the texts of the `1TO1` family (KEEP: the secret room reuses the 1:1 combat's texts): a guild WITH a bid must bid
// more than it did (`MORE_CURRENT_REQUEST`), a guild without one at least the minimum (`LIMIT_MIN`: FIX, the original never set its minimum, so only the server's refusal came); any other bid
// asks "You have applied with N Penya. Do you wish to add M Penya?" (OK / Cancel) and OK hands the amount to the request and closes the window. CLOSE destroys.

import {
  APP_SECRETROOM_OFFER,
  WIDC_ACCEPT,
  WIDC_CLOSE,
  WIDC_EDIT1,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT1TO1_LIMIT_MIN,
  TID_GAME_GUILDCOMBAT1TO1_MORE_CURRENT_REQUEST,
  TID_GAME_GUILDCOMBAT1TO1_MORE_REQUEST,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { I32_MAX, type OfferRequest, type SecretRoomHost } from './ports';
import { OFFER_MAX_DIGITS, parseBid } from './rows';

export interface OfferDeps {
  readonly host: SecretRoomHost;
}

export class WndSecretRoomOffer extends WndNeuz {
  readonly appId = APP_SECRETROOM_OFFER;
  readonly #request: OfferRequest;
  readonly #host: SecretRoomHost;
  #edit: EditCtrl | null = null;

  constructor(request: OfferRequest, deps: OfferDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
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

  /** `m_dwBackupGold`: the bid the new one is compared with (the minimum for a guild without a bid). */
  get backupGold(): number {
    return this.#request.req !== 0 ? this.#request.req : this.#request.min;
  }
  /** The amount the edit starts with: the current bid, or the minimum when there is none. */
  get preset(): number {
    return this.backupGold;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    if (this.#edit) {
      this.#edit.addWndStyle(EBS_NUMBER);
      this.#edit.setMaxLength(OFFER_MAX_DIGITS);
      this.#edit.setString(String(this.preset));
    }
    // the panel `secretroomdesc` (`SecretRoomDesc.inc`)
    if (this.#request.info !== '') this.infoText?.setString(this.#request.info);
    this.moveParentCenter();
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
    if (r.req !== 0) {
      if (value <= backup) {
        this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT1TO1_MORE_CURRENT_REQUEST));
        return null;
      }
    } else if (value < r.min) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT1TO1_LIMIT_MIN));
      return null;
    }
    const text =
      r.req === 0
        ? this.#host.fmt(TID_GAME_GUILDCOMBAT1TO1_MORE_REQUEST, 0, value)
        : this.#host.fmt(TID_GAME_GUILDCOMBAT1TO1_MORE_REQUEST, backup, value - backup);
    this.#host.confirm(
      text,
      () => {
        r.tender(value);
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
