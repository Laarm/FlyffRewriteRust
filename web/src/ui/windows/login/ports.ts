// Ports of the login-state window shells (Plan/42 §4.5, WP-UI-11): the READ MODELS (`*View`, observed through a
// `ViewSource`) and the ACTIONS the shells call. Plain TS interfaces without protobuf types; the implementations are the
// feature controllers of 45 (WP-CFEAT-02/03/04: `web/src/features/{login,charselect,charcreate}`), the shells are tested
// with mock ports only. Each action lists the legacy send function the controller behind it covers (ref/99 App. B).

/** An observable read model: `get()` is cheap and returns the same object until something changed. */
export interface ViewSource<T> {
  get(): T;
  subscribe(fn: () => void): () => void;
}

/** A constant view (tests, fixtures). */
export function constantView<T>(value: T): ViewSource<T> {
  return { get: () => value, subscribe: () => () => undefined };
}

/** A mutable view with a `set` (tests, fixtures). */
export function mutableView<T>(initial: T): ViewSource<T> & { set(value: T): void } {
  let current = initial;
  const fns = new Set<() => void>();
  return {
    get: () => current,
    subscribe(fn) {
      fns.add(fn);
      return () => {
        fns.delete(fn);
      };
    },
    set(value) {
      current = value;
      for (const fn of [...fns]) fn();
    },
  };
}

// ---- 3-D previews (41 §21.1 `PreviewRenderer`, WP-REND-06) ---------------------------------------------------------------

/** What a shell asks of the preview renderer for one canvas (the canvas is the shell's, the subject is the feature's). */
export interface PreviewRequest {
  readonly canvas: HTMLCanvasElement;
  /** The core windows' paper doll and portrait open `inventory` / `statusPortrait` through the same service (WP-CFEAT-11). */
  readonly preset:
    | 'charSelect'
    | 'charCreate'
    | 'inventory'
    | 'statusPortrait'
    | 'beautyHair'
    | 'hairKind'
    | 'faceKind'
    | 'changeSex'
    /** The model of the system pet in the pet status window and of the eat pet in the buff pet window (WP-UI-33, WP-UI-34). */
    | 'petStatus'
    | 'petKind0'
    | 'petKind1'
    | 'petKind2'
    | 'petKind3'
    | 'petKind4'
    | 'petKind5'
    | 'petKind6'
    | 'buffPet';
  /** The opaque subject of the preview (`MoverVisualState` of 41); the shell never looks inside. */
  readonly subject: unknown;
  /** The selected slot of the character list gets up and runs, the others sit (41 §21.2). */
  readonly selected?: boolean;
  /** `false` = the preview is drawn when it is ready and again on `invalidate` / `setSubject` only (the cards of the beauty shops); default `true`. */
  readonly animate?: boolean;
}

/** The handle of an open preview. */
export interface PreviewHandleLike {
  setSelected(selected: boolean): void;
  setSubject(subject: unknown): void;
  /** The extra `RotY` of the model in degrees (the turntable of the beauty shops: `RotationY( g_tmCurrent / 1000 )`); optional. */
  setRotation?(degrees: number): void;
  invalidate(): void;
  close(): void;
}

/** `host.previews` of 42 §8.12: `null` when no renderer exists (the canvas stays empty). */
export interface PreviewPort {
  open(request: PreviewRequest): PreviewHandleLike | null;
}

// ---- login (APP_LOGIN, ref/22 §17 B) ------------------------------------------------------------------------------------

export interface LoginView {
  /** The remembered account name (`flyff.login.rememberAccount`), `''` when none. */
  readonly account: string;
  readonly rememberAccount: boolean;
  /** `bootstrap.server_name`, shown in the title strip of the window (45 §3.1 step 3). */
  readonly serverName: string;
  /** The line under the fields: `TID_DIAG_0053` (maintenance / shutting down), `TID_DIAG_0041` (full), else `''`. */
  readonly statusText: string;
  /** `auth.allow_registration`: the Register button exists. */
  readonly registrationEnabled: boolean;
  /** `auth.register_url` (non-empty: an external link next to the button; `''` none). */
  readonly registerUrl: string;
  /** A login request is in flight: OK and Enter do nothing (45 §3.2 step 7). */
  readonly busy: boolean;
  /** `features.fast_login`: `false` slides the window in from the top in 400 ms (cosmetic), `true` shows it at once. */
  readonly fastLogin: boolean;
  /** The text of a box when OK is pressed with an empty account (`TID_DIAG_0005`). */
  readonly emptyAccountText: string;
}

export interface LoginActions {
  /**
   * OK: the account (trimmed, non-empty), the raw password and the "save account" check. The controller applies the client
   * rules (`TID_DIAG_0005` / `TID_DIAG_0030`), hashes, posts `/api/login` (legacy `SendCertify`).
   */
  login(account: string, password: string, rememberAccount: boolean): void;
  /** The Register button (only with `registrationEnabled`). */
  openRegister(): void;
}

// ---- registration panel (new; rendered with the APP_LOGIN skin, 45 §3.3) ------------------------------------------------

export interface RegisterView {
  readonly busy: boolean;
  /** The caption texts of the panel (new English texts: the layout has none). */
  readonly labels: {
    readonly title: string;
    readonly account: string;
    readonly password: string;
    readonly confirm: string;
    readonly email: string;
    readonly register: string;
    readonly cancel: string;
  };
}

export interface RegisterActions {
  /** Register: the controller compares the confirmation, validates and posts `/api/register` (then logs in). */
  submit(account: string, password: string, confirm: string, email: string): void;
  cancel(): void;
}

// ---- takeover prompt (APP_CLOSE_EXISTING_CONNECTION, 12 §14) -------------------------------------------------------------

export interface TakeoverView {
  /** `TID_DIAG_0065`: "This account is already connected. Would you like to cut the existing connection?" */
  readonly text: string;
}

export interface TakeoverActions {
  /** Yes: `login(..., force: true)` (legacy `CLOSE_EXISTING_CONNECTION`). */
  yes(): void;
  /** No: back to the fields. */
  no(): void;
}

// ---- character select (APP_SELECT_CHAR) -------------------------------------------------------------------------------------

export interface CharSlotView {
  readonly idPlayer: number;
  readonly name: string;
  readonly blocked: boolean;
  readonly level: number;
  readonly jobName: string;
  readonly str: number;
  readonly sta: number;
  readonly dex: number;
  readonly int: number;
  /** 0 male, 1 female (the hover text). */
  readonly idSex: number;
  /** The opaque preview subject (`MoverVisualState`). */
  readonly subject: unknown;
}

export interface CharSelectView {
  /** Three slots; `null` = empty. */
  readonly slots: readonly (CharSlotView | null)[];
  readonly selected: number;
  /** A lobby request is in flight: every button is disabled (45 §4.2). */
  readonly busy: boolean;
  /** `TID_GAME_WND_SELECT_CHARACTER_EMPTY` ("Available"). */
  readonly emptyLabel: string;
  /** The hover texts: `{name}`, `Lv. {level}`... built by the feature (texts bundle). */
  readonly infoText: (slot: CharSlotView) => string;
}

export interface CharSelectActions {
  /** A click on a slot or a left/right arrow (the controller wraps and handles blocked slots). */
  select(slot: number): void;
  /** Create on an empty slot. */
  create(slot: number): void;
  /** Delete on an occupied, non-blocked slot (opens the password dialog). */
  remove(slot: number): void;
  /** Start / double-click / Enter on an occupied, non-blocked slot (`SendPreJoin` + `SendJoin`). */
  start(slot: number): void;
  /** Back: close the WebSocket, drop the token, show the login window. */
  back(): void;
}

// ---- character create (APP_CREATE_CHAR) ------------------------------------------------------------------------------------

export interface CharCreateView {
  /** 0 male, 1 female. */
  readonly sex: number;
  readonly busy: boolean;
  /** The opaque subject of the preview (sex, hair, head, hair colour, starter equipment). */
  readonly subject: unknown;
  /** `gameplay.max_name_letters` (the name edit's maximum length). */
  readonly maxNameLength: number;
}

export interface CharCreateActions {
  setSex(sex: number): void;
  /** `-1` / `+1` with wrap-around (hair mesh 0..9). */
  stepHair(direction: -1 | 1): void;
  /** `-1` / `+1` with wrap-around (head mesh 0..10). */
  stepHead(direction: -1 | 1): void;
  /** OK: the name, the PIN and its confirmation as typed (the controller runs the pre-checks, `SendCreatePlayer`). */
  ok(name: string, pin: string, pinConfirm: string): void;
  /** Cancel: back to the character select. */
  cancel(): void;
}

// ---- character delete (APP_DELETE_CHAR) ---------------------------------------------------------------------------------------

export interface CharDeleteView {
  /** OK and Cancel are disabled until the reply (the legacy `DeletePlayer`). */
  readonly busy: boolean;
  /** Bumps whenever the key field must be cleared (a wrong password, 1319 / 112). */
  readonly clearNonce: number;
  /** The caption above the field (`auth.delete_requires_password`: the account password; else the legacy key). */
  readonly label: string;
}

export interface CharDeleteActions {
  /** OK: the typed key (the controller hashes it with the legacy salt and sends `DelPlayer`). */
  ok(key: string): void;
  cancel(): void;
}
