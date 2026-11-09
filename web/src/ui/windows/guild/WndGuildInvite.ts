// `WndGuildInvite` (`CWndGuildConfirm`, `src:_Interface/WndGuild.cpp:190-248`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-45): `APP_GUILD_INVATE` (240 x 144), the
// invitation to a guild: the guild name in the static `WIDC_STATIC3`, "Guild is inviting you. Will you join the Guild?", Yes (`WIDC_OK`, also Enter) and No (`WIDC_NO`). A normal
// window that opens centred horizontally 70 px from the top. Yes asks the caller to send `AddGuildMemberReq` and closes; No and the close button just close
// (KEEP, Plan/45 §23: the legacy client never sent `IgnoreGuildInvite`, the invitation expires on the server).

import { APP_GUILD_INVATE, WIDC_NO, WIDC_OK, WIDC_STATIC3 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { InviteRequest } from './ports';

/** `Move( ( rectRoot.right - width ) / 2, 70 )`. */
export const INVITE_Y = 70;

export class WndGuildInvite extends WndNeuz {
  readonly appId = APP_GUILD_INVATE;
  readonly #request: InviteRequest;
  #answered = false;

  constructor(request: InviteRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }
  get nameText(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC3);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.nameText?.setTitle(this.#request.guildName);
    const layout = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2),
      y: INVITE_Y,
    });
  }

  yes(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.yes();
    this.destroy();
  }

  no(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED || msg === Wnm.EN_RETURN) {
      if (id === WIDC_OK || msg === Wnm.EN_RETURN) {
        this.yes();
        return true;
      }
      if (id === WIDC_NO) {
        this.no();
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
