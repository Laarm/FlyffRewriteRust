// The login controller (Plan/45 §3, WP-CFEAT-02; 12 §6, §14, §16, §18.1): the `LoginActions` / `RegisterActions` /
// `TakeoverActions` behind the shells of `ui/windows/login`. It validates (through `createAuthActions`: account and password
// rules, the legacy hash, `POST /api/login`), shows the "Connecting" box, handles the duplicate-login prompt (code 103 ->
// the yes/no window -> `force: true`), the registration panel (`POST /api/register`, then the same login), the remembered
// account name (the only thing stored: no password, no hash, PL-129) and the notices of the login screen.

import { createAuthActions, type AuthActions, type LoginOutcome } from '../../app/authActions';
import { APP_CLOSE_EXISTING_CONNECTION, APP_LOGIN, APP_MESSAGEBOX } from '../../gen/defs/resdata';
import {
  TID_DIAG_0005,
  TID_DIAG_0030,
  TID_DIAG_0041,
  TID_DIAG_0043,
  TID_DIAG_0053,
  TID_DIAG_0064,
  TID_DIAG_0065,
  TID_DIAG_0068,
} from '../../gen/defs/text';
import { safeStorage } from '../../platform/storage';
import type { UiNotice } from '../../stores/screen';
import {
  constantView,
  type RegisterActions,
  type RegisterView,
} from '../../ui/windows/login/ports';
import type {
  LoginActions,
  LoginView,
  TakeoverView,
  ViewSource,
} from '../../ui/windows/login/ports';
import { ConnectingBox } from '../../ui/windows/login/ConnectingBox';
import { WndCloseExistingConnection } from '../../ui/windows/login/WndCloseExisting';
import { WndLogin } from '../../ui/windows/login/WndLogin';
import { WndRegister } from '../../ui/windows/login/WndRegister';
import type { FeatureContext } from '../common/module';
import { derivedView, localState } from '../common/view';

/** `localStorage` key of the remembered account NAME (12 §18.1; the password and its hash are never stored). */
export const REMEMBER_ACCOUNT_KEY = 'flyff.login.rememberAccount';

/** The instance key of the registration panel (a second instance of the `APP_LOGIN` layout). */
export const REGISTER_KEY = 'register';

export interface LoginController {
  readonly model: ViewSource<LoginView>;
  /** Opens (or focuses) the login window. */
  show(): void;
  /** Closes the login window, the registration panel and the takeover prompt. */
  hide(): void;
  /** One `APP_MESSAGEBOX` with the notice text (45 §3.4). */
  showNotice(notice: UiNotice): void;
  /** The "Connecting. Please wait." box (no buttons). */
  showConnecting(): void;
  closeConnecting(): void;
  readonly actions: LoginActions;
  /** What the registration panel calls (`submit` / `cancel`). */
  readonly registerActions: RegisterActions;
}

export interface LoginControllerDeps {
  /** Test seam: the auth actions (default: the real ones over the context). */
  readonly auth?: AuthActions;
  /** Test seam: timers for the 429 lock. */
  readonly setTimeout?: (fn: () => void, ms: number) => unknown;
}

const readRemembered = (): string => safeStorage.local.get(REMEMBER_ACCOUNT_KEY) ?? '';

/** Stores the account name when "save account" is checked, forgets it otherwise (never the password). */
export function persistRememberedAccount(remember: boolean, account: string): void {
  if (remember && account !== '') safeStorage.local.set(REMEMBER_ACCOUNT_KEY, account);
  else safeStorage.local.remove(REMEMBER_ACCOUNT_KEY);
}

export function createLoginController(
  fc: FeatureContext,
  deps: LoginControllerDeps = {},
): LoginController {
  const ctx = fc.app;
  const auth = deps.auth ?? createAuthActions(ctx);
  const schedule = deps.setTimeout ?? ((fn: () => void, ms: number) => setTimeout(fn, ms));
  const local = localState({ busy: false, locked: false, version: 0 });
  const session = ctx.stores.sessionStore;
  let box: ConnectingBox | null = null;

  const text = (tid: number): string => fc.texts.text(tid);

  const statusText = (): string => {
    const s = session.getState().server.status;
    if (s === 'maintenance' || s === 'shutting_down') return text(TID_DIAG_0053);
    if (s === 'full') return text(TID_DIAG_0041);
    return '';
  };

  const model = derivedView<LoginView>(() => {
    const st = session.getState();
    const account = readRemembered();
    return {
      account,
      rememberAccount: account !== '',
      serverName: st.server.name,
      statusText: statusText(),
      registrationEnabled: st.authRules?.allow_registration ?? false,
      registerUrl: st.authRules?.register_url ?? '',
      busy: local.get().busy || local.get().locked,
      fastLogin: ctx.features.features.fast_login,
      emptyAccountText: text(TID_DIAG_0005),
    };
  }, [session, local, ctx.bundles]);

  const registerModel = derivedView<RegisterView>(
    () => ({
      busy: local.get().busy,
      labels: {
        title: 'Register',
        account: 'Account',
        password: 'Password',
        confirm: 'Confirm',
        email: 'E-mail',
        register: 'Register',
        cancel: 'Cancel',
      },
    }),
    [local],
  );

  const closeApp = (app: number, key = ''): void => {
    const ui = fc.ui;
    if (!ui) return;
    ui.wm.close(app, key);
    ui.wm.flushDestroyed();
  };

  const showConnecting = (): void => {
    const ui = fc.ui;
    if (!ui) return;
    box = new ConnectingBox(text(TID_DIAG_0064), text(TID_DIAG_0068));
    ui.msgbox.openCustom(box);
  };

  const closeConnecting = (): void => {
    const ui = fc.ui;
    if (!ui || !box) return;
    const current = ui.wm.get(APP_MESSAGEBOX, 'main');
    if (current === box) {
      ui.wm.destroy(box);
      ui.wm.flushDestroyed();
    }
    box = null;
  };

  /** The flow after a request answered: `failed` (a notice shows through the screen state), `takeover`, `ok`. */
  const finish = (
    outcome: LoginOutcome,
    account: string,
    password: string,
    remember: boolean,
  ): void => {
    switch (outcome.kind) {
      case 'ok':
        persistRememberedAccount(remember, outcome.account);
        closeApp(APP_LOGIN, REGISTER_KEY);
        return;
      case 'takeover':
        closeConnecting();
        promptTakeover(account, password, remember);
        return;
      case 'failed': {
        // the state machine moved to `Login{notice}`; the screen controller shows the box. A throttle (429) also
        // disables OK for `retry_after_ms` (45 §3.2 step 6).
        const retry = outcome.failure?.retryAfterMs;
        if (retry !== undefined && retry > 0) {
          local.set({ locked: true });
          schedule(() => {
            local.set({ locked: false });
          }, retry);
        }
      }
    }
  };

  const run = async (
    account: string,
    password: string,
    remember: boolean,
    force: boolean,
  ): Promise<void> => {
    if (!fc.gate.tryAcquire('login')) return; // one request in flight (45 §3.2 step 7)
    local.set({ busy: true });
    showConnecting();
    try {
      const outcome = await auth.login(account, password, { force });
      finish(outcome, account, password, remember);
    } catch (e) {
      ctx.log.warn('auth', `login failed: ${e instanceof Error ? e.message : String(e)}`);
      ctx.machine.dispatch({ type: 'loginFailed', notice: { tid: TID_DIAG_0043 } });
    } finally {
      fc.gate.release('login');
      local.set({ busy: false });
      // the box stays while the socket opens (`Connecting`): the screen controller closes it at `CharSelect`
      if (ctx.machine.screen.s === 'Login') closeConnecting();
    }
  };

  const promptTakeover = (account: string, password: string, remember: boolean): void => {
    const ui = fc.ui;
    if (!ui) return;
    const view: ViewSource<TakeoverView> = constantView({ text: text(TID_DIAG_0065) });
    ui.wm.open(
      APP_CLOSE_EXISTING_CONNECTION,
      () =>
        new WndCloseExistingConnection(view, {
          yes: () => {
            void run(account, password, remember, true);
          },
          no: () => undefined,
        }),
    );
  };

  const registerWith = async (
    account: string,
    password: string,
    confirm: string,
    email: string,
  ): Promise<void> => {
    const ui = fc.ui;
    if (password !== confirm) {
      ui?.msgbox.open(text(TID_DIAG_0030));
      return;
    }
    if (!fc.gate.tryAcquire('login')) return;
    local.set({ busy: true });
    try {
      const outcome = await auth.register(account, password, email === '' ? undefined : email);
      if (outcome.kind === 'failed') {
        const msg = outcome.notice.text ?? text(outcome.notice.tid);
        ui?.msgbox.open(msg);
        return;
      }
      // registered and logged in with the same credentials (12 §6.5)
      finish(outcome.login, account, password, false);
    } catch (e) {
      ctx.log.warn('auth', `register failed: ${e instanceof Error ? e.message : String(e)}`);
      ui?.msgbox.open(text(TID_DIAG_0043));
    } finally {
      fc.gate.release('login');
      local.set({ busy: false });
    }
  };

  const registerActions: RegisterActions = {
    submit: (a, p, c, e) => {
      void registerWith(a, p, c, e);
    },
    cancel: () => {
      closeApp(APP_LOGIN, REGISTER_KEY);
    },
  };

  const actions: LoginActions = {
    login: (account, password, remember) => {
      void run(account, password, remember, false);
    },
    openRegister: () => {
      const ui = fc.ui;
      if (!ui) return;
      ui.wm.open(APP_LOGIN, () => new WndRegister(registerModel, registerActions), REGISTER_KEY);
    },
  };

  return {
    model,
    actions,
    registerActions,
    show() {
      const ui = fc.ui;
      if (!ui) return;
      ui.wm.open(APP_LOGIN, () => new WndLogin(model, actions));
    },
    hide() {
      closeApp(APP_CLOSE_EXISTING_CONNECTION);
      closeApp(APP_LOGIN, REGISTER_KEY);
      closeApp(APP_LOGIN);
    },
    showNotice(notice) {
      const ui = fc.ui;
      if (!ui) return;
      const msg = notice.text ?? text(notice.tid);
      ui.msgbox.open(msg);
    },
    showConnecting,
    closeConnecting,
  };
}
