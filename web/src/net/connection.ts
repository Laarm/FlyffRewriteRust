// The connection lifecycle (Plan/40 section 5.1, 5.6, 12.3, WP-CLIENT-12 and the reconnect loop of -24): one WebSocket at
// a time, the `Hello` handshake, what `HelloAck` and `Kick` mean (12 section 16.4: `closeCodes.ts` decides, the screen
// state machine acts), the `ServerTimeReq` cadence, client liveness, and the reconnect loop with the session token
// inside the grace window.
//
//   open(reattach)   close the previous socket and AWAIT its close (12 section 18.2: no spurious 103 on the next login),
//                    open `ws(s)://<host>/ws`, send `Hello{token, proto_hash, client_build, client_frame, reattach}`
//   HelloAck         state-machine event; on OK: limits to the sender, the first clock sample, `data_version` check,
//                    `ServerTimeReq` now, 4 more at 1 s, then every 10 s (and once when the tab becomes visible)
//   liveness         visible tab, session established, no inbound frame for 30 s (or `bufferedAmount` above 1 MiB for
//                    10 s): close with 4901 and reconnect; no app-level ping exists (D3)
//   reconnect        retries at 0.5, 1, 2, 4, 4, ... s with the same token while `now - lost < auth.token_grace_s`;
//                    stops on a refused `HelloAck`, a `Kick` without reconnect, or the grace running out
//
// Client close codes: 1000 (logout, quit, back, a new connection), 4900 (L2 resync), 4901 (liveness).

import { create } from '@bufbuild/protobuf';
import {
  HelloSchema,
  ServerTimeReqSchema,
  type HelloAck,
  type Kick,
  type ServerTime,
} from '../gen/proto/flyff/v1/session_pb';
import { HelloAck_Result } from '../gen/proto/flyff/v1/session_pb';
import type { SessionHost, StateMachine } from '../app/stateMachine';
import type { Logger } from '../platform/log';
import type { SessionKeys } from '../platform/session';
import { clientFrameAt, FRAME_MS, now as platformNow } from '../platform/time';
import type { Unsubscribe } from '../platform/visibility';
import type { Publisher } from '../stores/publisher';
import { CONNECTED_SCREENS } from '../stores/screen';
import { applyHelloAck, type SessionState } from '../stores/session';
import type { StoreApi } from 'zustand/vanilla';
import type { NetClient } from './client';
import { CloseCode } from './closeCodes';
import { n64 } from './int64';
import type { MsgCtx } from './registry';
import { openSocket, type Transport } from './socket';

/** `ServerTimeReq` cadence (Plan/40 section 5.6): right after `HelloAck`, `FAST_COUNT` more at `FAST_MS`, then `SLOW_MS`. */
export const CADENCE_FAST_COUNT = 4;
export const CADENCE_FAST_MS = 1000;
export const CADENCE_SLOW_MS = 10_000;
/** Liveness (Plan/40 section 5.1 rule 6, 7). */
export const LIVENESS_MS = 30_000;
export const LIVENESS_CHECK_MS = 1000;
export const BUFFERED_LIMIT_BYTES = 1024 * 1024;
export const BUFFERED_LIMIT_MS = 10_000;
/** Reconnect retry delays; the last one repeats (Plan/12 section 13.3). */
export const RECONNECT_BACKOFF_MS: readonly number[] = [500, 1000, 2000, 4000];
/** A reconnect attempt that has neither been answered nor closed after this long is abandoned. */
export const ATTEMPT_TIMEOUT_MS = 10_000;
/** `close()` waits at most this for the socket's close event. */
export const CLOSE_WAIT_MS = 5000;
/** A close that arrives while frames are still queued waits for the drain, at most this long. */
export const PENDING_CLOSE_MS = 1000;
/** The default token grace (`auth.token_grace_s`, Plan/12 section 4). */
export const DEFAULT_GRACE_MS = 60_000;

export interface ConnectionTimers {
  setTimeout(cb: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
}

export interface ConnectionDeps {
  readonly net: Pick<NetClient, 'attach' | 'detach' | 'sender' | 'clock' | 'inbound'> & {
    readonly registry: Pick<NetClient['registry'], 'beginJoin'>;
  };
  readonly machine: Pick<StateMachine, 'dispatch' | 'screen' | 'subscribe'>;
  readonly sessionStore: Pick<StoreApi<SessionState>, 'getState' | 'setState'>;
  readonly publisher?: Pick<Publisher, 'register'>;
  readonly session: Pick<SessionKeys, 'token' | 'clear' | 'setInWorld'>;
  readonly build: { readonly id: string; readonly protoHash: string };
  readonly log: Pick<Logger, 'info' | 'warn' | 'error'>;
  readonly now?: () => number;
  /** The WebSocket URL (default `ws(s)://<location.host>/ws`). */
  readonly url?: () => string;
  /** Test seam: the transport factory (default `openSocket`). */
  readonly openTransport?: (url: string) => Transport;
  /** `auth.token_grace_s` in ms (default 60 s). */
  readonly graceMs?: () => number;
  /** The `data_version` of the loaded bundle manifest (`''` = not loaded: no check). */
  readonly dataVersion?: () => string;
  /** `reloadOnce('proto' | 'data')`. */
  readonly requestReload?: (reason: 'proto' | 'data') => void;
  readonly timers?: ConnectionTimers;
  readonly isHidden?: () => boolean;
  readonly subscribeVisibility?: (cb: (hidden: boolean) => void) => Unsubscribe;
}

/** The statistics the probe and the tests read. */
export interface ConnectionStats {
  readonly opens: number;
  readonly helloAcks: number;
  readonly kicks: number;
  readonly serverTimeReqs: number;
  readonly livenessCloses: number;
  readonly resyncs: number;
  readonly reconnectAttempts: number;
  /** Frames the server sent that arrived after the last `lastInbound` stamp. */
  readonly established: boolean;
}

export interface Connection {
  /** Closes the previous socket (awaiting its close), opens a new one and sends `Hello`. */
  open(reattach: boolean): Promise<void>;
  /** Closes with `code` and resolves when the socket is closed (at once without a socket). */
  close(code: number): Promise<void>;
  /** L2: close with 4900 and reconnect with `Hello{reattach: wasInGame}`. */
  resync(reason: string): void;
  /** Test hook `dropSocket`: the socket closes like a lost network connection (1006). */
  simulateDrop(): void;
  /** The watchdog: `Connecting` took too long. */
  abandon(reason: string): void;
  /** Starts the reconnect loop (the machine's `beginReconnect` effect). */
  beginReconnect(wasInGame: boolean): void;
  /** Delays the next reconnect attempt (the machine's `scheduleReconnect` effect, `Kick.reconnect_after_ms`). */
  scheduleReconnect(afterMs: number): void;
  /** The tab became visible (background pump `onReturn`): restart the silence timer and sync the clock. */
  onVisible(): void;
  /** The tab went hidden or came back: liveness checks pause while hidden. */
  onHiddenChange(hidden: boolean): void;
  /** After a drain (registered by `app/connectionModule.ts`): a socket close that waited for its queued frames is classified now. */
  onDrainEnd(): void;
  /** Registry handlers (phase Core), see `app/connectionModule.ts`. */
  handleHelloAck(ack: HelloAck, ctx: MsgCtx): void;
  handleKick(kick: Kick): void;
  handleServerTime(msg: ServerTime, ctx: MsgCtx): void;
  /** Assigns the effects of the state machine that belong to the connection. */
  install(host: Partial<SessionHost>): void;
  readonly stats: ConnectionStats;
  dispose(): void;
}

interface Conn {
  readonly id: number;
  readonly transport: Transport;
  /** Opened by the reconnect loop. */
  readonly attempt: boolean;
  closed: boolean;
  /** Set when WE close it: the close is no news. */
  intentional: boolean;
}

interface ReconnectState {
  readonly wasInGame: boolean;
  lostAt: number;
  attempt: number;
  timer: unknown;
  watch: unknown;
}

function defaultUrl(): string {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${location.host}/ws`;
}

export function createConnection(deps: ConnectionDeps): Connection {
  const now = deps.now ?? platformNow;
  const timers: ConnectionTimers = deps.timers ?? {
    setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
    clearTimeout: (id) => {
      globalThis.clearTimeout(id as number);
    },
  };
  const isHidden =
    deps.isHidden ??
    (() => typeof document !== 'undefined' && document.visibilityState === 'hidden');
  const openTransport = deps.openTransport ?? ((url: string) => openSocket(url));
  const urlOf = deps.url ?? defaultUrl;
  const graceMs = deps.graceMs ?? (() => DEFAULT_GRACE_MS);
  const { net, machine, log } = deps;

  let conn: Conn | null = null;
  let connId = 0;
  /** Counts `open` calls: a call that was overtaken while it waited for the previous close gives up. */
  let openSeq = 0;
  let closing: Promise<void> = Promise.resolve();
  let established = false;
  let lastInbound = 0;
  let bufferedSince: number | null = null;
  let cadenceTimer: unknown = null;
  let cadenceIndex = 0;
  let livenessTimer: unknown = null;
  let pendingClose: { conn: Conn; code: number } | null = null;
  let pendingTimer: unknown = null;
  let reconnect: ReconnectState | null = null;
  const counters = {
    opens: 0,
    helloAcks: 0,
    kicks: 0,
    serverTimeReqs: 0,
    livenessCloses: 0,
    resyncs: 0,
    reconnectAttempts: 0,
  };
  const offs: Unsubscribe[] = [];

  // ---- socket ---------------------------------------------------------------------------------------------

  /** Closes the current socket (marked as ours); the returned promise also covers an earlier close in progress. */
  const closeCurrent = (code: number, reason: string): Promise<void> => {
    const c = conn;
    if (!c) return closing;
    stopSession();
    c.intentional = true;
    conn = null;
    net.detach();
    const done = new Promise<void>((resolve) => {
      if (c.closed) {
        resolve();
        return;
      }
      let finished = false;
      let off: Unsubscribe = () => undefined;
      let timer: unknown = null;
      const finish = (): void => {
        if (finished) return;
        finished = true;
        off();
        if (timer !== null) timers.clearTimeout(timer);
        resolve();
      };
      off = c.transport.onClose(finish);
      timer = timers.setTimeout(finish, CLOSE_WAIT_MS);
      try {
        c.transport.close(code, reason);
      } catch {
        finish();
      }
    });
    closing = Promise.all([closing, done]).then(() => undefined);
    return closing;
  };

  const sendHello = (token: string, reattach: boolean): void => {
    net.sender.send({
      case: 'hello',
      value: create(HelloSchema, {
        token,
        protoHash: deps.build.protoHash,
        clientBuild: deps.build.id,
        clientFrame: clientFrameAt(now()),
        reattach,
      }),
    });
    net.sender.flush();
  };

  const open = async (reattach: boolean, attempt: boolean): Promise<void> => {
    openSeq += 1;
    const mine = openSeq;
    await closeCurrent(CloseCode.Normal, 'new connection');
    if (mine !== openSeq) return; // a newer open() took over while this one waited
    const token = deps.session.token();
    if (token === null) {
      log.warn('conn', 'no session token: back to the login screen');
      machine.dispatch({ type: 'socketClosedWith', code: CloseCode.BadToken });
      return;
    }
    const url = urlOf();
    connId += 1;
    counters.opens += 1;
    const transport = openTransport(url);
    const c: Conn = { id: connId, transport, attempt, closed: false, intentional: false };
    conn = c;
    established = false;
    lastInbound = now();
    pendingClose = null;
    net.attach(transport);
    const hello = (): void => {
      if (conn === c) sendHello(token, reattach);
    };
    transport.onOpen(hello);
    if (transport.isOpen) hello(); // a transport that is open already (a test double) sends at once
    transport.onMessage(() => {
      if (conn === c) lastInbound = now();
    });
    transport.onClose((code) => {
      onClosed(c, code);
    });
    if (attempt && reconnect) {
      if (reconnect.watch !== null) timers.clearTimeout(reconnect.watch);
      reconnect.watch = timers.setTimeout(() => {
        if (conn === c && !established) {
          log.warn('conn', 'reconnect attempt timed out');
          void closeCurrent(CloseCode.Normal, 'attempt timeout').then(attemptFailed);
        }
      }, ATTEMPT_TIMEOUT_MS);
    }
  };

  const onClosed = (c: Conn, code: number): void => {
    c.closed = true;
    if (conn !== c) return; // an older socket, or one we closed on purpose
    stopSession();
    net.detach();
    conn = null;
    log.info('conn', `socket closed (${String(code)})`);
    if (net.inbound.depth > 0) {
      // frames (a `Kick`, a `HelloAck` refusal) are still queued: classify the close after they were dispatched
      pendingClose = { conn: c, code };
      if (pendingTimer !== null) timers.clearTimeout(pendingTimer);
      pendingTimer = timers.setTimeout(flushPendingClose, PENDING_CLOSE_MS);
      return;
    }
    handleClose(c, code);
  };

  const flushPendingClose = (): void => {
    if (pendingTimer !== null) timers.clearTimeout(pendingTimer);
    pendingTimer = null;
    const p = pendingClose;
    pendingClose = null;
    if (p) handleClose(p.conn, p.code);
  };

  const handleClose = (c: Conn, code: number): void => {
    const screen = machine.screen.s;
    if (screen === 'Reconnecting') {
      // only the loop's own attempts end an attempt; the socket that was lost belongs to the loop already
      if (c.attempt) attemptFailed();
      return;
    }
    if (!CONNECTED_SCREENS.has(screen)) return; // Login, Boot, Reloading, Fatal: a Kick or refusal already moved us
    if (code === CloseCode.BadToken || code === CloseCode.ReloadRequired) {
      machine.dispatch({ type: 'socketClosedWith', code });
      return;
    }
    machine.dispatch({ type: 'socketLost', now: now() });
  };

  // ---- clock cadence and liveness -----------------------------------------------------------------------

  const stopSession = (): void => {
    established = false;
    bufferedSince = null;
    if (cadenceTimer !== null) timers.clearTimeout(cadenceTimer);
    if (livenessTimer !== null) timers.clearTimeout(livenessTimer);
    cadenceTimer = null;
    livenessTimer = null;
  };

  /**
   * The gateway derives the stamp offset of the session as `server frame at ARRIVAL - client_frame`, which includes the
   * one-way delay of the request: every `PlayerMoveState` would be stamped late by that delay and the other players'
   * copies of this player would lag by `delay x speed` (found by the bot's peer-divergence KPI K3 with 200 ms of injected
   * latency: 1.2 m at running speed). A request therefore announces the frame the client will have reached when it
   * arrives, `client_frame + rtt / 2` (the round trip measured so far; no shift before the first sample); the clock
   * estimator gets the frame the request was sent at back (`timeShifts`, keyed by what was announced).
   */
  const timeShifts = new Map<number, number>();
  const sendServerTimeReq = (): void => {
    if (!established || !conn) return;
    counters.serverTimeReqs += 1;
    const t = now();
    const shift = net.clock.synced ? Math.round(net.clock.rttMs / 2 / FRAME_MS) : 0;
    const announced = (clientFrameAt(t) + shift) >>> 0;
    timeShifts.set(announced, shift);
    if (timeShifts.size > 16) {
      const oldest = timeShifts.keys().next();
      if (oldest.done !== true) timeShifts.delete(oldest.value);
    }
    net.sender.send({
      case: 'serverTimeReq',
      value: create(ServerTimeReqSchema, { clientFrame: announced, clientPerfMs: t }),
    });
    net.sender.flush();
  };

  const scheduleCadence = (): void => {
    const delay = cadenceIndex < CADENCE_FAST_COUNT ? CADENCE_FAST_MS : CADENCE_SLOW_MS;
    cadenceTimer = timers.setTimeout(() => {
      cadenceTimer = null;
      if (!established) return;
      cadenceIndex += 1;
      sendServerTimeReq();
      scheduleCadence();
    }, delay);
  };

  const dead = (why: string): void => {
    counters.livenessCloses += 1;
    log.warn('conn', `${why}: closing with ${String(CloseCode.ClientLiveness)}`);
    void closeCurrent(CloseCode.ClientLiveness, why);
    machine.dispatch({ type: 'socketLost', now: now() });
  };

  const checkLiveness = (): void => {
    livenessTimer = null;
    const c = conn;
    if (!established || !c) return;
    if (!isHidden()) {
      const t = now();
      if (t - lastInbound >= LIVENESS_MS) {
        dead('no inbound frame for 30 s');
        return;
      }
      if (c.transport.bufferedAmount > BUFFERED_LIMIT_BYTES) {
        bufferedSince ??= t;
        if (t - bufferedSince >= BUFFERED_LIMIT_MS) {
          dead('uplink stalled (bufferedAmount above 1 MiB for 10 s)');
          return;
        }
      } else {
        bufferedSince = null;
      }
    }
    livenessTimer = timers.setTimeout(checkLiveness, LIVENESS_CHECK_MS);
  };

  const startSession = (): void => {
    established = true;
    lastInbound = now();
    bufferedSince = null;
    cadenceIndex = 0;
    sendServerTimeReq();
    scheduleCadence();
    if (livenessTimer !== null) timers.clearTimeout(livenessTimer);
    livenessTimer = timers.setTimeout(checkLiveness, LIVENESS_CHECK_MS);
  };

  // ---- reconnect loop -----------------------------------------------------------------------------------

  const cancelReconnect = (): void => {
    const r = reconnect;
    reconnect = null;
    if (!r) return;
    if (r.timer !== null) timers.clearTimeout(r.timer);
    if (r.watch !== null) timers.clearTimeout(r.watch);
  };

  const scheduleAttempt = (delay: number): void => {
    const r = reconnect;
    if (!r) return;
    if (r.timer !== null) timers.clearTimeout(r.timer);
    r.timer = timers.setTimeout(runAttempt, delay);
  };

  const runAttempt = (): void => {
    const r = reconnect;
    if (!r) return;
    r.timer = null;
    if (machine.screen.s !== 'Reconnecting') {
      cancelReconnect();
      return;
    }
    if (now() - r.lostAt >= graceMs()) {
      log.warn('conn', 'token grace exceeded: back to the login screen');
      cancelReconnect();
      machine.dispatch({ type: 'reconnectGraceExceeded' });
      return;
    }
    r.attempt += 1;
    counters.reconnectAttempts += 1;
    machine.dispatch({ type: 'reconnectAttempt', attempt: r.attempt });
    void open(r.wasInGame, true);
  };

  /** An attempt ended without a `HelloAck`: wait for the next backoff step. */
  const attemptFailed = (): void => {
    const r = reconnect;
    if (!r || machine.screen.s !== 'Reconnecting') return;
    if (r.watch !== null) timers.clearTimeout(r.watch);
    r.watch = null;
    const delay =
      RECONNECT_BACKOFF_MS[Math.min(r.attempt, RECONNECT_BACKOFF_MS.length - 1)] ?? 4000;
    scheduleAttempt(delay);
  };

  offs.push(
    machine.subscribe(() => {
      // the machine's CURRENT screen, not the one of the notification: a resync dispatches from inside an effect
      if (machine.screen.s !== 'Reconnecting') cancelReconnect();
    }),
  );
  // visibility: the context wires the background pump's `onReturn`/`onHiddenChange`; a standalone connection may
  // subscribe through `subscribeVisibility`
  if (deps.subscribeVisibility) {
    offs.push(
      deps.subscribeVisibility((hidden) => {
        api.onHiddenChange(hidden);
        if (!hidden) api.onVisible();
      }),
    );
  }

  // 1 Hz publisher: rtt, clock offset (session.net)
  if (deps.publisher) {
    offs.push(
      deps.publisher.register('connection.net', 1, () => {
        const cur = deps.sessionStore.getState().net;
        const next = {
          rttMs: Math.round(net.clock.rttMs),
          offsetFrames: Math.round(net.clock.offset),
          synced: net.clock.synced,
        };
        if (
          cur.rttMs !== next.rttMs ||
          cur.offsetFrames !== next.offsetFrames ||
          cur.synced !== next.synced
        ) {
          deps.sessionStore.setState({ net: next });
        }
      }),
    );
  }

  // ---- messages -----------------------------------------------------------------------------------------

  const api: Connection = {
    open: (reattach) => open(reattach, false),

    async close(code) {
      cancelReconnect();
      await closeCurrent(code, code === CloseCode.Normal ? 'bye' : 'client close');
    },

    resync(reason) {
      if (machine.screen.s === 'Reconnecting') return;
      counters.resyncs += 1;
      log.warn('conn', `resync: ${reason}`);
      void closeCurrent(CloseCode.ClientResync, reason);
      machine.dispatch({ type: 'socketLost', now: now() });
    },

    simulateDrop() {
      const c = conn;
      if (!c) return;
      // like a lost connection: the close is NOT ours
      c.transport.close(1006, 'simulated drop');
    },

    abandon(reason) {
      if (!conn) {
        machine.dispatch({ type: 'socketLost', now: now() });
        return;
      }
      log.warn('conn', `abandoning the connection: ${reason}`);
      void closeCurrent(CloseCode.ClientLiveness, reason);
      machine.dispatch({ type: 'socketLost', now: now() });
    },

    beginReconnect(wasInGame) {
      cancelReconnect();
      stopSession();
      // a socket that is still around (liveness, a stuck connect) is closed before the first attempt
      if (conn) void closeCurrent(CloseCode.ClientLiveness, 'reconnect');
      reconnect = { wasInGame, lostAt: now(), attempt: 0, timer: null, watch: null };
      scheduleAttempt(RECONNECT_BACKOFF_MS[0] ?? 500);
    },

    scheduleReconnect(afterMs) {
      const r = reconnect;
      if (!r) return;
      // a server restart: wait that long, and the grace window starts after the wait
      r.lostAt = now() + afterMs;
      scheduleAttempt(Math.max(afterMs, RECONNECT_BACKOFF_MS[0] ?? 500));
    },

    onVisible() {
      lastInbound = now();
      bufferedSince = null;
      sendServerTimeReq();
    },

    onHiddenChange(hidden) {
      // the liveness check skips hidden tabs by itself; a stall that began before is not held against the tab
      if (hidden) bufferedSince = null;
    },

    onDrainEnd: flushPendingClose,

    handleHelloAck(ack, ctx) {
      counters.helloAcks += 1;
      const reattached = ack.reattached;
      machine.dispatch({ type: 'helloAck', result: ack.result, reattached, now: now() });
      if (ack.result !== HelloAck_Result.OK) {
        if (reconnect) cancelReconnect();
        return;
      }
      if (machine.screen.s === 'Connecting' || !CONNECTED_SCREENS.has(machine.screen.s)) {
        // an ack the machine did not accept (late or duplicate): it starts no session
        log.warn('conn', `HelloAck ignored in ${machine.screen.s}`);
        return;
      }
      cancelReconnect();
      const limits = ack.limits;
      if (limits) {
        net.sender.setLimits({
          maxMsgBytes: limits.maxClientMsgBytes,
          msgsPerSec: limits.clientMsgsPerSec,
          burst: limits.clientMsgsBurst,
        });
      }
      net.clock.reset();
      net.clock.addSample({
        clientFrameEcho: ack.clientFrameEcho,
        clientFrameNow: clientFrameAt(ctx.recvAt),
        serverFrame: n64(ack.serverFrame),
        serverUnixMs: n64(ack.serverUnixMs),
        perfNowMs: ctx.recvAt,
      });
      deps.sessionStore.setState(applyHelloAck(deps.sessionStore.getState(), ack));
      const loaded = deps.dataVersion?.() ?? '';
      if (ack.dataVersion !== '' && loaded !== '' && ack.dataVersion !== loaded) {
        log.warn('conn', `data_version ${ack.dataVersion} differs from the loaded ${loaded}`);
        deps.requestReload?.('data');
      }
      startSession();
    },

    handleKick(kick) {
      counters.kicks += 1;
      log.warn(
        'conn',
        `Kick{reason ${String(kick.reason)}, tid ${String(kick.tid)}, reconnect_after_ms ${String(kick.reconnectAfterMs)}} ${kick.detail}`,
      );
      machine.dispatch({
        type: 'kick',
        reason: kick.reason,
        tid: kick.tid,
        reconnectAfterMs: kick.reconnectAfterMs,
      });
    },

    handleServerTime(msg, ctx) {
      const shift = timeShifts.get(msg.clientFrameEcho) ?? 0;
      timeShifts.delete(msg.clientFrameEcho);
      net.clock.addSample({
        clientFrameEcho: (msg.clientFrameEcho - shift) >>> 0,
        clientFrameNow: clientFrameAt(ctx.recvAt),
        serverFrame: n64(msg.serverFrame),
        serverUnixMs: n64(msg.serverUnixMs),
        perfNowMs: ctx.recvAt,
        ...(msg.clientPerfMsEcho > 0 ? { clientPerfEchoMs: msg.clientPerfMsEcho } : {}),
      });
    },

    install(host) {
      host.openSocket = (reattach) => open(reattach, false);
      host.closeSocket = (code) => api.close(code);
      host.beginReconnect = (wasInGame) => {
        api.beginReconnect(wasInGame);
      };
      host.scheduleReconnect = (afterMs) => {
        api.scheduleReconnect(afterMs);
      };
      host.resync = (reason) => {
        api.resync(reason);
      };
      host.beginJoin = () => {
        net.registry.beginJoin();
      };
      host.dropToken = () => {
        deps.session.clear();
      };
      host.setInWorld = (marker) => {
        deps.session.setInWorld(marker);
      };
    },

    get stats(): ConnectionStats {
      return { ...counters, established };
    },

    dispose() {
      cancelReconnect();
      stopSession();
      if (pendingTimer !== null) timers.clearTimeout(pendingTimer);
      for (const off of offs.splice(0)) off();
      if (conn) {
        conn.intentional = true;
        conn.transport.close(CloseCode.Normal, 'dispose');
        conn = null;
      }
    },
  };
  return api;
}
