// `WndPostDeleteConfirm` (`CWndPostDeleteConfirm`, `src:_Interface/WndField.cpp:15449-15528`; WP-UI-27): `APP_POST_DELETE_CONFIRM` (256 x 144), modal. The text
// asks whether the mail should go ("items and penya in this mail will be deleted as well"); Yes sends the removal and closes this window and the
// reader, No closes this window.

import { APP_POST_DELETE_CONFIRM, WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import { TID_MAIL_DELETE_CONFIRM } from '../../../gen/defs/text';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_MODAL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DeleteConfirmRequest } from './ports';

export interface DeleteConfirmDeps {
  readonly getText: (tid: number) => string;
}

export class WndPostDeleteConfirm extends WndNeuz {
  readonly appId = APP_POST_DELETE_CONFIRM;
  readonly #deps: DeleteConfirmDeps;
  readonly #request: DeleteConfirmRequest;
  #text: TextCtrl | null = null;

  constructor(deps: DeleteConfirmDeps, request: DeleteConfirmRequest) {
    super();
    this.#deps = deps;
    this.#request = request;
    this.putRegInfo = false;
  }

  /**
   * `WBS_MODAL`, like the reader it is asked from: with the reader modal and focused, a window that is not modal never gets a press (the manager routes every
   * press to the focused modal frame; found by the Playwright flow, where the pointer could not answer the question).
   */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  /** The question (`WIDC_TEXT1`). */
  get text(): TextCtrl | null {
    return this.#text;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const text = this.getDlgItem<TextCtrl>('WIDC_TEXT1');
    text?.setString(this.#deps.getText(TID_MAIL_DELETE_CONFIRM));
    this.#text = text;
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_YES) {
      this.#request.yes();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_NO) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
