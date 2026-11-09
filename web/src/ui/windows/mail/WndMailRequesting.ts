// `WndMailRequestingBox` (`CWndMailRequestingBox`, `src:_Interface/WndMailRequestingBox.cpp`; WP-UI-27): `APP_REQUESTING_POST` (192 x 192), the modal box
// that covers the post window while the mailbox is on its way. It has no close button and does not move. The request button (the send picture) is
// disabled for 5 seconds after the window opened and after each press; a press asks for the mailbox again (`SendQueryMailBox`). The third press that
// was allowed (the counter counts presses, even refused ones) lets the box go the next time its timer runs out - the way out when the answer never
// comes. The post window closes the box when the mailbox arrives.

import { APP_REQUESTING_POST } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_MODAL, WBS_MOVE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { DESTRUCTION_PRESSES, REQUEST_WAIT_MS } from './names';
import type { RequestingBoxRequest } from './ports';

export class WndMailRequesting extends WndNeuz {
  readonly appId = APP_REQUESTING_POST;
  readonly #request: RequestingBoxRequest;
  #button: Button | null = null;
  #enableAt: number | null = null;
  #presses = 0;

  constructor(request: RequestingBoxRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  /** The request button (`WIDC_BUTTON_REQUEST`). */
  get requestButton(): Button | null {
    return this.#button;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `m_bNoCloseButton = TRUE`, `DelWndStyle( WBS_MOVE )`
    this.delWndStyle(WBS_MOVE);
    this.titleBar?.setVisible(false);
    this.#button = this.getDlgItem<Button>('WIDC_BUTTON_REQUEST');
    this.#button?.enableWindow(false);
    this.#enableAt = this.env.host.now() + REQUEST_WAIT_MS;
    this.moveParentCenter();
  }

  override process(now: number): void {
    // `Process`: the timer ran out - the box may go after three presses, the button is free again
    if (this.#enableAt !== null && now >= this.#enableAt) {
      if (this.#presses >= DESTRUCTION_PRESSES) this.destroy();
      this.#button?.enableWindow(true);
      this.#enableAt = null;
    }
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === this.#button?.id) {
      this.#presses += 1;
      const b = this.#button;
      if (!b.isWindowEnabled()) return true;
      this.#request.request();
      this.#enableAt = this.env.host.now() + REQUEST_WAIT_MS;
      b.enableWindow(false);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
