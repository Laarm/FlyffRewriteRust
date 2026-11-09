// sessionStore (Plan/40 section 6.2.1; scope `never`): screen state, connection, account and server
// info, overlay and the 1 Hz network statistics. Only the state machine writes `screen`.
// Deviation from the plan sketch: absent optional values are `null` (JSON-serialisable and compatible
// with `exactOptionalPropertyTypes`, ST6).

import type { HelloAck } from '../gen/proto/flyff/v1/session_pb';
import type { ScreenState } from './screen';

/** `Bootstrap['status']` (12 section 6.2). */
export type ServerStatus = 'open' | 'maintenance' | 'shutting_down' | 'full';

/** `bootstrap.auth` (12 section 18.6). */
export interface BootstrapAuth {
  readonly md5_salt: string;
  readonly allow_registration: boolean;
  readonly register_url: string;
  readonly account_min_len: number;
  readonly account_max_len: number;
  readonly password_min_len: number;
  readonly password_max_len: number;
  readonly delete_requires_password: boolean;
  readonly token_grace_s: number;
}

export type ConnectionState = 'idle' | 'connecting' | 'open' | 'reconnecting' | 'closed';
export type OverlayKind = 'none' | 'connecting' | 'reconnecting' | 'preparing' | 'loading';

/** Preparing-assets progress (PL-130, 24 section 12). */
export interface PrepareState {
  readonly active: boolean;
  readonly percent: number;
  readonly kind: string;
  readonly current: string;
  readonly since: number;
}

export interface SessionState {
  /** Section 4.1; written only by the state machine (SM1). */
  readonly screen: ScreenState;
  readonly connection: ConnectionState;
  readonly account: string | null;
  /** `HelloAck.authority` (AUTH_* letter code; UI gating only). */
  readonly authority: number;
  readonly server: { readonly name: string; readonly build: string; readonly status: ServerStatus };
  readonly authRules: BootstrapAuth | null;
  readonly reattached: boolean;
  readonly overlay: OverlayKind;
  readonly prepare: PrepareState | null;
  /** Published at 1 Hz. */
  readonly net: { readonly rttMs: number; readonly offsetFrames: number; readonly synced: boolean };
}

export function initialSessionState(): SessionState {
  return {
    screen: { s: 'Boot' },
    connection: 'idle',
    account: null,
    authority: 0,
    server: { name: '', build: '', status: 'open' },
    authRules: null,
    reattached: false,
    overlay: 'none',
    prepare: null,
    net: { rttMs: 0, offsetFrames: 0, synced: false },
  };
}

/** Reducer for `HelloAck{OK}` (40 section 5.1 step 3): account, authority, server name/build, reattach flag. */
export function applyHelloAck(state: SessionState, msg: HelloAck): SessionState {
  return {
    ...state,
    account: msg.account === '' ? state.account : msg.account,
    authority: msg.authority,
    server: { ...state.server, name: msg.serverName, build: msg.serverBuild },
    reattached: msg.reattached,
  };
}
