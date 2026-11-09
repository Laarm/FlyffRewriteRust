// `WndLogin` (`CWndLogin`, `src:_Interface/WndTitle.cpp:144-660`, ref/22 §17 B, Plan/42 §4.5, 45 §3): `APP_LOGIN`
// (`WndTile09`, 288 x 256 layout; the height is forced to 200 by `initDialog`). The shell ports `OnInitialUpdate` (the edits
// with their tab stops, the masked password, the default OK button, the remembered account and the focus rule, the English
// variant of the layout: the About / Credit / Prologue buttons hidden, OK and Quit moved up) and `OnChildNotify` (OK, the
// "save account" check) to calls of `LoginActions`.
//
// Differences from the original that the plan prescribes:
//  * no "save password" check box (`__SAVE_PASSWORD` is dropped: no password or hash is ever stored, PL-129),
//  * Quit closes the client in the original; a browser cannot close its tab, so the button is hidden and a Register button
//    takes its place when `auth.allow_registration` (45 §3.3),
//  * the server name is shown in the title strip, a status line below the buttons (45 §3.1 step 3),
//  * `features.fast_login = false` slides the window in from the top in 400 ms (cosmetic).

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
} from '../../../gen/defs/resdata';
import { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import { Static } from '../../controls/Static';
import { EBS_PASSWORD, WBS_KEY, Wnm } from '../../layout/styles';
import { MessageBoxService } from '../../msgbox/MessageBoxService';
import type { Shortcut } from '../../dnd/shortcut';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LoginActions, LoginView, ViewSource } from './ports';

/** The id of the code-created Register button and of the status line (above every layout id of the window). */
export const WIDC_LOGIN_REGISTER = 9001;
export const WIDC_LOGIN_STATUS = 9002;

/** Duration of the entry animation with `features.fast_login = false` (45 §3.1 step 2). */
export const LOGIN_SLIDE_MS = 400;

/** The English-variant positions of `OnInitialUpdate` (`pOk->Move(72, 105); pQuit->Move(72, 135)`). */
export const LOGIN_OK_AT = { x: 72, y: 105 } as const;
export const LOGIN_SECOND_AT = { x: 72, y: 135 } as const;

export class WndLogin extends WndNeuz {
  readonly appId = APP_LOGIN;
  readonly #model: ViewSource<LoginView>;
  readonly #actions: LoginActions;
  #off: (() => void) | null = null;
  #account: EditCtrl | null = null;
  #password: EditCtrl | null = null;
  #ok: Button | null = null;
  #check: Button | null = null;
  #register: Button | null = null;
  #status: Static | null = null;
  #sliding = false;
  #slideFrom = Number.NaN;
  #slideTo = 0;
  #slideStart = Number.NaN;

  constructor(model: ViewSource<LoginView>, actions: LoginActions) {
    super();
    this.#model = model;
    this.#actions = actions;
    this.putRegInfo = false; // `SetPutRegInfo(FALSE)`
  }

  /** `InitDialog(APP_LOGIN, WBS_KEY, ...)`: Enter presses the default button, Tab walks the edits. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_KEY | extraStyle);
  }

  /** The account edit (tests, the controller's focus calls). */
  get accountEdit(): EditCtrl | null {
    return this.#account;
  }
  get passwordEdit(): EditCtrl | null {
    return this.#password;
  }
  get okButton(): Button | null {
    return this.#ok;
  }
  get registerButton(): Button | null {
    return this.#register;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const m = this.#model.get();
    const account = this.getDlgItem<EditCtrl>(WIDC_ACCOUNT);
    const password = this.getDlgItem<EditCtrl>(WIDC_PASSWORD);
    const check = this.getDlgItem<Button>(WIDC_CHECK1);
    const ok = this.getDlgItem<Button>(WIDC_OK);
    this.#account = account;
    this.#password = password;
    this.#check = check;
    this.#ok = ok;
    account?.setTabStop(true);
    password?.addWndStyle(EBS_PASSWORD);
    password?.setTabStop(true);
    ok?.setDefault(true);
    if (account && m.account !== '') account.setString(m.account);
    check?.setCheck(m.rememberAccount);

    this.moveParentCenter();

    // `__SAVE_PASSWORD` is dropped (PL-129): no "save password" box
    const savePassword = this.getDlgItem<Button>(WIDC_CHECK2);
    savePassword?.setVisible(false);
    savePassword?.enableWindow(false);
    // English variant: About, Credit and Prologue are hidden, OK and Quit move up
    for (const id of [WIDC_ABOUT, WIDC_CREDIT, WIDC_PROLOGUE]) {
      const b = this.getDlgItem<Button>(id);
      b?.enableWindow(false);
      b?.setVisible(false);
    }
    ok?.move(LOGIN_OK_AT);
    const quit = this.getDlgItem<Button>(WIDC_QUIT);
    quit?.move(LOGIN_SECOND_AT);
    // a browser cannot exit: Quit is hidden (the Register button takes its place)
    quit?.enableWindow(false);
    quit?.setVisible(false);

    // the Register button (45 §3.3) and the status line (45 §3.1)
    const register = new Button();
    this.addControl(register, WIDC_LOGIN_REGISTER, {
      l: LOGIN_SECOND_AT.x,
      t: LOGIN_SECOND_AT.y,
      r: LOGIN_SECOND_AT.x + 140,
      b: LOGIN_SECOND_AT.y + 20,
    });
    register.setTitle('Register');
    this.#register = register;
    const status = new Static();
    this.addControl(status, WIDC_LOGIN_STATUS, { l: 10, t: 160, r: 270, b: 176 }, 0);
    status.setTextColor(0xffc02020);
    this.#status = status;

    this.#apply(m);
    this.#off = this.#model.subscribe(() => {
      this.#apply(this.#model.get());
    });
    this.#startSlide(m);
    // the last control created holds the focus (KEEP): the focus rule comes after every control exists. A remembered
    // account puts the focus on the password.
    if (m.account !== '') password?.setFocus();
    else account?.setFocus();
  }

  /** Diffing setters only: nothing is written when the view did not change. */
  #apply(m: LoginView): void {
    this.setTitle(m.serverName === '' ? this.getTitle() : m.serverName);
    this.#ok?.enableWindow(!m.busy);
    this.#status?.setTitle(m.statusText);
    if (this.#register) {
      const show = m.registrationEnabled;
      this.#register.setVisible(show);
      this.#register.enableWindow(show && !m.busy);
    }
  }

  /** `fast_login = false`: park the window above the viewport until the first `process` starts the 400 ms slide. */
  #startSlide(m: LoginView): void {
    if (m.fastLogin) return;
    this.#slideTo = this.rectWindow.t;
    this.#slideFrom = Number.NaN;
    this.#slideStart = Number.NaN;
    this.#sliding = true;
    (this as { processEveryFrame?: boolean }).processEveryFrame = true;
    // above the viewport (the frame still grows to a multiple of 16 px after this call)
    this.move({ x: this.rectWindow.l, y: -(this.rectWindow.b - this.rectWindow.t) - 16 });
  }

  /** The 400 ms slide-in of `fast_login = false`. */
  override process(now: number): void {
    if (!this.#sliding) return;
    if (Number.isNaN(this.#slideStart)) {
      this.#slideStart = now;
      // the frame was rounded up to 16 px after `onInitialUpdate`: take the final height now
      this.#slideFrom = -(this.rectWindow.b - this.rectWindow.t);
    }
    const t = Math.min(1, (now - this.#slideStart) / LOGIN_SLIDE_MS);
    const y = Math.round(this.#slideFrom + (this.#slideTo - this.#slideFrom) * t);
    this.move({ x: this.rectWindow.l, y });
    if (t >= 1) {
      this.#sliding = false;
      (this as { processEveryFrame?: boolean }).processEveryFrame = false;
    }
  }

  /** `OnChildNotify`: the "save account" check and OK. */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.#submit();
        return true;
      }
      if (id === WIDC_LOGIN_REGISTER) {
        this.#actions.openRegister();
        return true;
      }
    }
    if (msg === Wnm.EN_RETURN && (id === WIDC_ACCOUNT || id === WIDC_PASSWORD)) {
      // Enter inside an edit presses the default button (the key handling of a `WBS_KEY` frame)
      this.#submit();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** OK: an empty account opens the standard box (`TID_DIAG_0005`) and calls no port; else `login(...)`. */
  #submit(): void {
    const m = this.#model.get();
    if (m.busy) return;
    const account = (this.#account?.getString() ?? '').trim();
    if (account === '') {
      new MessageBoxService(this.env.wm).open(m.emptyAccountText);
      this.#account?.setFocus();
      return;
    }
    this.#actions.login(account, this.#password?.getString() ?? '', this.#check?.getCheck() === 1);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    super.onDestroy();
  }
}
