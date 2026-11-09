// ScreenState (Plan/40 section 4.1): the type of `sessionStore.screen`. The state machine
// (`app/stateMachine.ts`) is the only writer (rule SM1); the type lives here so that the stores layer
// does not depend on `app/`. `app/stateMachine.ts` re-exports it.

/** A notice shown on the login screen (12 section 16.4/16.5): a TID, or built-in text when `tid` is 0. */
export interface UiNotice {
  /** `TID_*` numeric id of the client text (0 = use `text`). */
  readonly tid: number;
  /** Built-in text for messages that have no TID (12 section 16.1 codes 2002 and 2005). */
  readonly text?: string;
}

/** What the loading screen waits for (Plan/40 section 9.8 step 5; the conditions are evaluated by WP-CLIENT-23). */
export interface LoadingGate {
  /** `SpawnSelf` was processed (SM3). */
  readonly selfSpawned: boolean;
  /** Collision, terrain, own model and own-land statics are ready (section 9.8 step 5). */
  readonly assetsReady: boolean;
  /** `now()` when the gate started (for the 60 s timeout, section 9.8 step 6). */
  readonly startedAtMs: number;
}

/** Content of the L4 fatal screen (Plan/40 section 12.2). */
export interface FatalInfo {
  readonly reason: string;
  readonly detail?: string;
}

export type LoadingReason = 'enter' | 'worldChange' | 'reattach';

/** The client screen states (Plan/40 section 4.1). */
export type ScreenState =
  | { readonly s: 'Boot' }
  | { readonly s: 'Login'; readonly notice?: UiNotice }
  | { readonly s: 'Connecting'; readonly reattach: boolean }
  | { readonly s: 'CharSelect'; readonly busy: boolean }
  | { readonly s: 'CharCreate'; readonly slot: number; readonly busy: boolean }
  | { readonly s: 'Entering'; readonly slot: number; readonly idPlayer: number }
  | {
      readonly s: 'Loading';
      readonly reason: LoadingReason;
      readonly worldId: number;
      readonly gate: LoadingGate;
    }
  | { readonly s: 'InGame' }
  | { readonly s: 'Leaving' }
  | {
      readonly s: 'Reconnecting';
      readonly attempt: number;
      readonly since: number;
      readonly wasInGame: boolean;
    }
  | { readonly s: 'Reloading'; readonly reason: string }
  | { readonly s: 'Fatal'; readonly error: FatalInfo };

/** The discriminant values of {@link ScreenState}. */
export type ScreenName = ScreenState['s'];

/** States in which a session (WebSocket) is established or being established. */
export const CONNECTED_SCREENS: ReadonlySet<ScreenName> = new Set<ScreenName>([
  'Connecting',
  'CharSelect',
  'CharCreate',
  'Entering',
  'Loading',
  'InGame',
  'Leaving',
]);
