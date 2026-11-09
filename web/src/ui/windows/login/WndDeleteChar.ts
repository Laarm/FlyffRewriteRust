// `WndDeleteChar` (`CWndDeleteChar`, `src:_Interface/WndTitle.cpp:1222-1425`, ref/22 §17 B, Plan/42 §4.5, 45 §4.4):
// `APP_DELETE_CHAR` (modal, 272 x 208): one masked field for the account password (the legacy "key"), OK and Cancel. OK
// hands the typed text to the controller, which hashes it and sends `DelPlayer`; the buttons stay disabled until the reply
// (`CharDeleteView.busy`), a wrong password clears the field (`clearNonce`).

import {
  APP_DELETE_CHAR,
  WIDC_CANCEL,
  WIDC_CONTROL1,
  WIDC_EDIT1,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import { EBS_PASSWORD, WBS_KEY, WBS_MODAL, WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { CharDeleteActions, CharDeleteView, ViewSource } from './ports';

export class WndDeleteChar extends WndNeuz {
  readonly appId = APP_DELETE_CHAR;
  readonly #model: ViewSource<CharDeleteView>;
  readonly #actions: CharDeleteActions;
  #edit: EditCtrl | null = null;
  #ok: Button | null = null;
  #cancel: Button | null = null;
  #label: Static | null = null;
  #nonce = 0;
  #off: (() => void) | null = null;

  constructor(model: ViewSource<CharDeleteView>, actions: CharDeleteActions) {
    super();
    this.#model = model;
    this.#actions = actions;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_DELETE_CHAR, WBS_MODAL)`; Enter presses OK. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
  }

  get keyEdit(): EditCtrl | null {
    return this.#edit;
  }
  get okButton(): Button | null {
    return this.#ok;
  }
  get cancelButton(): Button | null {
    return this.#cancel;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#ok = this.getDlgItem<Button>(WIDC_OK);
    this.#cancel = this.getDlgItem<Button>(WIDC_CANCEL);
    this.#label = this.getDlgItem<Static>(WIDC_CONTROL1);
    this.#edit?.addWndStyle(EBS_PASSWORD); // `AddWndStyle(EBS_PASSWORD | EBS_AUTOHSCROLL)`
    this.#edit?.setTabStop(true);
    this.#ok?.setDefault(true);
    this.#edit?.setFocus();
    this.moveParentCenter();
    this.#apply(this.#model.get());
    this.#off = this.#model.subscribe(() => {
      this.#apply(this.#model.get());
    });
  }

  #apply(m: CharDeleteView): void {
    this.#ok?.enableWindow(!m.busy);
    this.#cancel?.enableWindow(!m.busy);
    if (m.label !== '' && this.#label) this.#label.setTitle(m.label);
    if (m.clearNonce !== this.#nonce) {
      this.#nonce = m.clearNonce;
      this.#edit?.setString('');
      this.#edit?.setFocus();
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED || msg === Wnm.EN_RETURN) {
      const busy = this.#model.get().busy;
      if (id === WIDC_OK || (msg === Wnm.EN_RETURN && id === WIDC_EDIT1)) {
        if (!busy) this.#actions.ok(this.#edit?.getString() ?? '');
        return true;
      }
      if (id === WIDC_CANCEL || id === WTBID_CLOSE) {
        if (!busy) this.#actions.cancel();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    super.onDestroy();
  }
}
