// Close codes and the `HelloAck.Result` / `Kick.Reason` -> client handling table (Plan/12 section 16.4,
// Plan/40 section 5.1). Pure: the connection module (WP-CLIENT-12) feeds the outcome to the state machine.

import { TID_DIAG_0015, TID_DIAG_0023, TID_DIAG_0041, TID_DIAG_0053 } from '../gen/defs/text';
import { HelloAck_Result, Kick_Reason } from '../gen/proto/flyff/v1/session_pb';
import type { UiNotice } from '../stores/screen';

/** WebSocket close codes used by the client and the server (Plan/11 section 1.5, Plan/40 section 5.1). */
export const CloseCode = {
  /** Normal close: logout, quit, back to login. */
  Normal: 1000,
  GoingAway: 1001,
  MessageTooBig: 1009,
  Abnormal: 1006,
  ServerError: 1011,
  /** Server close after a Kick: `4000 + Kick.Reason`. */
  KickBase: 4000,
  /** Client: L2 resync (Plan/40 section 12.2). */
  ClientResync: 4900,
  /** Client: no inbound frame for 30 s in a visible tab (section 5.1 rule 6). */
  ClientLiveness: 4901,
  /** Server: `4000 + RELOAD_REQUIRED`. */
  ReloadRequired: 4012,
  /** Server: `4000 + SESSION_EXPIRED` (bad token). */
  BadToken: 4013,
} as const;

/**
 * The login-screen text after `Kick{SHUTDOWN}`. The server has just logged everybody out for a restart (the in-game
 * countdown told the player so), so the client goes back to the login screen instead of waiting in "Reconnecting"
 * for a server that needs minutes (WP-OPS-12). No TID says "restarting" (`TID_DIAG_0053` is "Service currently
 * unavailable..."), so this is the built-in text of the notice; a `Kick.tid` from the server overrides it.
 */
export const SERVER_RESTARTING_TEXT =
  'The server is restarting. Please log in again in a few minutes.';

/** What the client does after a `HelloAck` failure or a `Kick` (12 section 16.4). */
export interface SessionOutcome {
  /** The notice for the login screen (`undefined`: no text). */
  readonly notice?: UiNotice;
  /** Run the reconnect policy (12 section 13.3) instead of going to the login screen. */
  readonly reconnect: boolean;
  /** Reload the page once with a cache bust (`reloadOnce('proto')`). */
  readonly reload: boolean;
  /** Forget the session token. */
  readonly dropToken: boolean;
}

const LOGIN = (tid: number | null, dropToken = true): SessionOutcome => ({
  ...(tid === null ? {} : { notice: { tid } }),
  reconnect: false,
  reload: false,
  dropToken,
});

/** `HelloAck` results other than OK (12 section 16.4). */
export function helloAckOutcome(result: HelloAck_Result): SessionOutcome | null {
  switch (result) {
    case HelloAck_Result.OK:
      return null;
    case HelloAck_Result.RELOAD_REQUIRED:
      return { reconnect: false, reload: true, dropToken: false };
    case HelloAck_Result.BAD_TOKEN:
      return LOGIN(null);
    case HelloAck_Result.SERVER_FULL:
      // the token stays valid: the player may retry
      return LOGIN(TID_DIAG_0041, false);
    case HelloAck_Result.SHUTTING_DOWN:
      return LOGIN(TID_DIAG_0053, false);
    case HelloAck_Result.BANNED:
      return LOGIN(TID_DIAG_0015);
    case HelloAck_Result.UNSPECIFIED:
      return LOGIN(TID_DIAG_0023);
  }
}

/**
 * `Kick{reason, tid, reconnect_after_ms}` (12 section 16.4). `Kick.tid` (non-zero) overrides the default
 * text of the reason. `SLOW_CONSUMER`, `SERVER_ERROR` and a non-zero `reconnect_after_ms` run the
 * reconnect policy; `DUPLICATE_LOGIN` never reconnects (no two tabs stealing the session forever).
 * `SHUTDOWN` never reconnects either, whatever `reconnect_after_ms` says: the player is back at the login screen
 * with the "server restarting" notice and the token is dropped (the restarted server does not know it).
 */
export function kickOutcome(
  reason: Kick_Reason,
  tid: number,
  reconnectAfterMs: number,
): SessionOutcome {
  const text = (fallback: number): UiNotice => ({ tid: tid !== 0 ? tid : fallback });
  switch (reason) {
    case Kick_Reason.RELOAD_REQUIRED:
      return { reconnect: false, reload: true, dropToken: false };
    case Kick_Reason.SESSION_EXPIRED:
      return LOGIN(null);
    case Kick_Reason.BANNED:
      return { notice: text(TID_DIAG_0015), reconnect: false, reload: false, dropToken: true };
    case Kick_Reason.SHUTDOWN:
      return {
        notice: tid !== 0 ? { tid } : { tid: 0, text: SERVER_RESTARTING_TEXT },
        reconnect: false,
        reload: false,
        dropToken: true,
      };
    case Kick_Reason.SLOW_CONSUMER:
    case Kick_Reason.SERVER_ERROR:
      return { notice: text(TID_DIAG_0023), reconnect: true, reload: false, dropToken: false };
    case Kick_Reason.DUPLICATE_LOGIN:
    case Kick_Reason.TIMEOUT:
    case Kick_Reason.GM_KICK:
    case Kick_Reason.PROTOCOL_ERROR:
    case Kick_Reason.INVALID_STATE:
    case Kick_Reason.RATE_LIMIT:
    case Kick_Reason.MESSAGE_TOO_BIG:
    case Kick_Reason.UNSPECIFIED:
      return reconnectAfterMs > 0 && reason !== Kick_Reason.DUPLICATE_LOGIN
        ? { notice: text(TID_DIAG_0023), reconnect: true, reload: false, dropToken: false }
        : { notice: text(TID_DIAG_0023), reconnect: false, reload: false, dropToken: true };
  }
}

/** The WS close code the server uses after `Kick{reason}`. */
export function kickCloseCode(reason: Kick_Reason): number {
  return CloseCode.KickBase + reason;
}
