// `WndRegister` (new, 45 §3.3): the registration panel. The original has no such window (accounts were created on a web
// page); the panel is rendered with the `APP_LOGIN` skin and reuses the account and password rows of that layout, adds the
// confirmation and the e-mail rows and the OK / Cancel buttons (`ButtOk.tga`, `ButtCancel.tga`). It is modal and a second
// instance of `APP_LOGIN` (instance key `register`), so the login window stays below.

import {
  APP_LOGIN,
  WIDC_ABOUT,
  WIDC_ACCOUNT,
  WIDC_CHECK1,
  WIDC_CHECK2,
  WIDC_CREDIT,
  WIDC_OK,
  WIDC_PASSWORD,
  WIDC_PROLOGUE,
  WIDC_QUIT,
  WIDC_STATIC2,
  WIDC_STATIC3,
} from '../../../gen/defs/resdata';
import { Button } from '../../controls/Button';
import { EditCtrl } from '../../controls/EditCtrl';
import { Static } from '../../controls/Static';
import type { Rect } from '../../layout/geometry';
import { EBS_PASSWORD, WBS_KEY, WBS_MODAL, WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { RegisterActions, RegisterView, ViewSource } from './ports';

const ID_CONFIRM = 9201;
const ID_EMAIL = 9202;
const ID_LABEL_CONFIRM = 9203;
const ID_LABEL_EMAIL = 9204;
const ID_SUBMIT = 9205;
const ID_CANCEL = 9206;
/** The skin of every edit of the login layout. */
const EDIT_SKIN = 'WndEditTile00.tga';

const row = (y: number): Rect => ({ l: 90, t: y, r: 249, b: y + 19 });
const label = (y: number): Rect => ({ l: 10, t: y, r: 89, b: y + 19 });

export class WndRegister extends WndNeuz {
  readonly appId = APP_LOGIN;
  override readonly instanceKey = 'register';
  readonly #model: ViewSource<RegisterView>;
  readonly #actions: RegisterActions;
  #account: EditCtrl | null = null;
  #password: EditCtrl | null = null;
  #confirm: EditCtrl | null = null;
  #email: EditCtrl | null = null;
  #submitButton: Button | null = null;
  #cancelButton: Button | null = null;
  #off: (() => void) | null = null;

  constructor(model: ViewSource<RegisterView>, actions: RegisterActions) {
    super();
    this.#model = model;
    this.#actions = actions;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
  }

  get accountEdit(): EditCtrl | null {
    return this.#account;
  }
  get passwordEdit(): EditCtrl | null {
    return this.#password;
  }
  get confirmEdit(): EditCtrl | null {
    return this.#confirm;
  }
  get emailEdit(): EditCtrl | null {
    return this.#email;
  }
  get submitButton(): Button | null {
    return this.#submitButton;
  }
  get cancelButton(): Button | null {
    return this.#cancelButton;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const m = this.#model.get();
    // the layout rows that are reused: account and password
    for (const id of [
      WIDC_OK,
      WIDC_QUIT,
      WIDC_ABOUT,
      WIDC_CREDIT,
      WIDC_PROLOGUE,
      WIDC_CHECK1,
      WIDC_CHECK2,
    ]) {
      const c = this.getDlgItem(id);
      c?.enableWindow(false);
      c?.setVisible(false);
    }
    const account = this.getDlgItem<EditCtrl>(WIDC_ACCOUNT);
    const password = this.getDlgItem<EditCtrl>(WIDC_PASSWORD);
    this.#account = account;
    this.#password = password;
    account?.setTabStop(true);
    password?.addWndStyle(EBS_PASSWORD);
    password?.setTabStop(true);
    this.getDlgItem<Static>(WIDC_STATIC3)?.setTitle(m.labels.account);
    this.getDlgItem<Static>(WIDC_STATIC2)?.setTitle(m.labels.password);
    // the added rows: confirmation (masked) and e-mail
    this.#addRow(ID_LABEL_CONFIRM, label(80), m.labels.confirm);
    this.#addRow(ID_LABEL_EMAIL, label(110), m.labels.email);
    const confirm = new EditCtrl();
    this.addControl(confirm, ID_CONFIRM, row(80));
    confirm.setSkin(EDIT_SKIN, true);
    confirm.addWndStyle(EBS_PASSWORD);
    confirm.setTabStop(true);
    this.#confirm = confirm;
    const email = new EditCtrl();
    this.addControl(email, ID_EMAIL, row(110));
    email.setSkin(EDIT_SKIN, true);
    email.setTabStop(true);
    this.#email = email;
    // OK / Cancel with the sprites of the other dialogs
    const ok = new Button();
    this.addControl(ok, ID_SUBMIT, { l: 56, t: 146, r: 128, b: 166 });
    ok.setTexture('ButtOk.tga');
    ok.setDefault(true);
    this.#submitButton = ok;
    const cancel = new Button();
    this.addControl(cancel, ID_CANCEL, { l: 152, t: 146, r: 224, b: 166 });
    cancel.setTexture('ButtCancel.tga');
    this.#cancelButton = cancel;
    this.moveParentCenter();
    account?.setFocus();
    this.#apply(m);
    this.#off = this.#model.subscribe(() => {
      this.#apply(this.#model.get());
    });
  }

  #addRow(id: number, rect: Rect, text: string): Static {
    const s = new Static();
    this.addControl(s, id, rect, 0);
    s.setTitle(text);
    return s;
  }

  #apply(m: RegisterView): void {
    this.setTitle(m.labels.title);
    this.#submitButton?.enableWindow(!m.busy);
    this.#cancelButton?.enableWindow(!m.busy);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const busy = this.#model.get().busy;
    if (msg === Wnm.CLICKED) {
      if (id === ID_SUBMIT) {
        if (!busy) this.#submit();
        return true;
      }
      if (id === ID_CANCEL || id === WTBID_CLOSE) {
        if (!busy) this.#actions.cancel();
        return true;
      }
    }
    if (
      msg === Wnm.EN_RETURN &&
      (id === WIDC_ACCOUNT || id === WIDC_PASSWORD || id === ID_CONFIRM || id === ID_EMAIL)
    ) {
      if (!busy) this.#submit();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  #submit(): void {
    this.#actions.submit(
      (this.#account?.getString() ?? '').trim(),
      this.#password?.getString() ?? '',
      this.#confirm?.getString() ?? '',
      (this.#email?.getString() ?? '').trim(),
    );
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    super.onDestroy();
  }
}
