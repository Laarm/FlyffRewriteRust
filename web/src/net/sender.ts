// Network sender (Plan/40 section 5.2, S1-S6): the one place client messages leave the page. One
// `ClientEnvelope` per WebSocket message; size guard (S1), token bucket mirroring `HelloAck.limits` (S2),
// FIFO order with in-place coalescing (S3), `flush()` once per loop iteration (S4), `not_connected` while
// there is no open transport and queued messages dropped on close (S5). An optional send gate (`NetSenderOptions.gate`,
// BFIX1 #021) refuses a message in `send` (never in `flush`): the client wires it from the screen so that no game message
// leaves while the gateway would kick or drop it (CharSelect, Leaving, ...).

import { create, toJson, type JsonValue } from '@bufbuild/protobuf';
import { ClientEnvelopeSchema } from '../gen/proto/flyff/v1/envelope_pb';
import type { Logger } from '../platform/log';
import { now as defaultNow } from '../platform/time';
import { encodeClient, type ClientMsg } from './codec';

export interface SendOptions {
  /** May be dropped under rate pressure (only PERIODIC movement messages). */
  readonly droppable?: boolean;
  /** Keep only the newest pending message of this key (it replaces its predecessor in place). */
  readonly coalesce?: 'move' | 'angle';
}

export type SendResult = 'queued' | 'dropped' | 'too_big' | 'not_connected';

/** The part of a `Transport` the sender writes to. */
export interface SenderTransport {
  send(bytes: Uint8Array): void;
  readonly isOpen: boolean;
}

/** Session limits (`HelloAck.limits`, Plan/11 section 1.2). */
export interface SenderLimits {
  readonly maxMsgBytes: number;
  readonly msgsPerSec: number;
  readonly burst: number;
}

export const DEFAULT_SENDER_LIMITS: SenderLimits = {
  maxMsgBytes: 8192,
  msgsPerSec: 60,
  burst: 120,
};

/** Bound of the pending FIFO (S2). */
export const MAX_QUEUED = 256;

/** Entries kept for `recent()` (probe `test.sentLog`). */
const SENT_LOG = 100;

export interface SenderStats {
  /** Messages written to the transport, by case name. */
  readonly sent: Readonly<Record<string, number>>;
  /** Messages dropped (droppable under pressure, queue overflow). */
  readonly dropped: number;
  readonly tooBig: number;
  /** Currently queued messages. */
  readonly queued: number;
  /** Messages merged into a pending message with the same coalesce key. */
  readonly coalesced: number;
  /** `send` calls refused because there was no open transport. */
  readonly notConnected: number;
  /** Messages the encoder rejected. */
  readonly encodeErrors: number;
  /** `send` calls the send gate refused (`NetSenderOptions.gate`). */
  readonly gated: number;
}

export interface NetSender {
  send(msg: ClientMsg, opts?: SendOptions): SendResult;
  /** Writes queued messages in order while the token bucket allows (once per loop iteration, S4). */
  flush(): void;
  /** Applies `HelloAck.limits`; zero or missing values keep the current limit. */
  setLimits(limits: Partial<{ maxMsgBytes: number; msgsPerSec: number; burst: number }>): void;
  readonly limits: SenderLimits;
  /** Drops every queued message (S5: on close; the re-attach join batch resynchronises state). */
  clear(): void;
  /** The last sent envelopes as canonical JSON (`Hello.token` redacted), oldest first. */
  recent(n?: number): { case: string; json: JsonValue }[];
  readonly stats: SenderStats;
}

export interface NetSenderOptions {
  /** The transport to write to; `null`/closed = not connected. */
  readonly transport: () => SenderTransport | null;
  readonly now?: () => number;
  readonly log?: Pick<Logger, 'warn' | 'error'> & Partial<Pick<Logger, 'info'>>;
  /**
   * The send gate: `false` refuses the message (`send` returns `'dropped'`, counted in `stats.gated` and logged at info
   * level). Evaluated ONLY in `send`, never in `flush`: a message accepted earlier (for example the `returnToCharSelect` of
   * the logout, sent while in game and flushed after the `leave` event) must still go out.
   */
  readonly gate?: (msg: ClientMsg) => boolean;
  readonly limits?: Partial<SenderLimits>;
  /** Throw on a message above the size cap instead of returning `too_big` (dev builds, S1). */
  readonly throwOnTooBig?: boolean;
  /** `?debug=net`: receives every sent message. */
  readonly inspector?: { onSend(msg: ClientMsg): void };
}

interface Pending {
  bytes: Uint8Array;
  msg: ClientMsg;
  readonly caseName: string;
  readonly droppable: boolean;
  readonly coalesceKey: string | null;
}

function caseOf(msg: ClientMsg): string {
  return msg.case ?? 'empty';
}

/** Canonical JSON of one envelope with the session token removed. */
export function envelopeJson(msg: ClientMsg): JsonValue {
  const json = toJson(ClientEnvelopeSchema, create(ClientEnvelopeSchema, { msg }));
  if (typeof json === 'object' && json !== null && !Array.isArray(json)) {
    const hello = json['hello'];
    if (typeof hello === 'object' && hello !== null && !Array.isArray(hello) && 'token' in hello) {
      return { ...json, hello: { ...hello, token: '<redacted>' } };
    }
  }
  return json;
}

export function createNetSender(options: NetSenderOptions): NetSender {
  const now = options.now ?? defaultNow;
  let limits: SenderLimits = { ...DEFAULT_SENDER_LIMITS, ...options.limits };
  let tokens = limits.burst;
  let lastRefill = now();

  const queue: Pending[] = [];
  const coalescing = new Map<string, Pending>();
  const ring: { msg: ClientMsg }[] = [];
  const stats = {
    sent: {} as Record<string, number>,
    dropped: 0,
    tooBig: 0,
    coalesced: 0,
    notConnected: 0,
    encodeErrors: 0,
    gated: 0,
  };

  const refill = (): void => {
    const t = now();
    const dt = t - lastRefill;
    lastRefill = t;
    if (dt > 0) tokens = Math.min(limits.burst, tokens + (dt / 1000) * limits.msgsPerSec);
  };

  const connected = (): SenderTransport | null => {
    const t = options.transport();
    return t?.isOpen === true ? t : null;
  };

  const forget = (p: Pending): void => {
    if (p.coalesceKey !== null && coalescing.get(p.coalesceKey) === p)
      coalescing.delete(p.coalesceKey);
  };

  const clear = (): void => {
    queue.length = 0;
    coalescing.clear();
  };

  return {
    send(msg, opts = {}) {
      if (!connected()) {
        stats.notConnected += 1;
        return 'not_connected';
      }
      if (options.gate && !options.gate(msg)) {
        stats.gated += 1;
        options.log?.info?.('sender', `${caseOf(msg)} refused by the send gate`);
        return 'dropped';
      }
      let bytes: Uint8Array;
      try {
        bytes = encodeClient(msg);
      } catch (error) {
        stats.encodeErrors += 1;
        options.log?.error(
          'sender',
          `cannot encode ${caseOf(msg)}: ${error instanceof Error ? error.message : String(error)}`,
        );
        return 'dropped';
      }
      if (bytes.byteLength > limits.maxMsgBytes) {
        stats.tooBig += 1;
        const text = `${caseOf(msg)} is ${String(bytes.byteLength)} bytes, above the ${String(limits.maxMsgBytes)} byte cap; not sent`;
        options.log?.error('sender', text);
        if (options.throwOnTooBig) throw new Error(text);
        return 'too_big';
      }
      const droppable = opts.droppable === true;
      const key = opts.coalesce ?? null;

      if (key !== null) {
        const pending = coalescing.get(key);
        if (pending) {
          pending.bytes = bytes;
          pending.msg = msg;
          stats.coalesced += 1;
          return 'queued';
        }
      }

      refill();
      // a droppable message that would have to wait behind others is dropped instead (S2)
      if (droppable && queue.length >= Math.floor(tokens)) {
        stats.dropped += 1;
        return 'dropped';
      }
      if (queue.length >= MAX_QUEUED) {
        const victim = queue.findIndex((p) => p.droppable);
        if (victim < 0) {
          stats.dropped += 1;
          options.log?.error(
            'sender',
            `send queue full (${String(MAX_QUEUED)}): ${caseOf(msg)} dropped`,
          );
          return 'dropped';
        }
        const [removed] = queue.splice(victim, 1);
        if (removed) forget(removed);
        stats.dropped += 1;
      }
      const entry: Pending = { bytes, msg, caseName: caseOf(msg), droppable, coalesceKey: key };
      queue.push(entry);
      if (key !== null) coalescing.set(key, entry);
      return 'queued';
    },
    flush() {
      const transport = connected();
      if (!transport) {
        // S5: queued messages are discarded while not connected
        if (queue.length > 0) clear();
        return;
      }
      refill();
      let written = 0;
      while (written < queue.length && tokens >= 1) {
        const p = queue[written];
        if (!p) break;
        tokens -= 1;
        forget(p);
        transport.send(p.bytes);
        stats.sent[p.caseName] = (stats.sent[p.caseName] ?? 0) + 1;
        ring.push({ msg: p.msg });
        if (ring.length > SENT_LOG) ring.shift();
        try {
          options.inspector?.onSend(p.msg);
        } catch {
          // the inspector is a debugging aid: it must never break the send path
        }
        written += 1;
      }
      if (written > 0) queue.splice(0, written);
    },
    setLimits(next) {
      limits = {
        maxMsgBytes:
          next.maxMsgBytes !== undefined && next.maxMsgBytes > 0
            ? next.maxMsgBytes
            : limits.maxMsgBytes,
        msgsPerSec:
          next.msgsPerSec !== undefined && next.msgsPerSec > 0
            ? next.msgsPerSec
            : limits.msgsPerSec,
        burst: next.burst !== undefined && next.burst > 0 ? next.burst : limits.burst,
      };
      tokens = Math.min(tokens, limits.burst);
    },
    get limits() {
      return limits;
    },
    clear,
    recent(n = SENT_LOG) {
      return ring
        .slice(Math.max(0, ring.length - n))
        .map(({ msg }) => ({ case: caseOf(msg), json: envelopeJson(msg) }));
    },
    get stats() {
      return {
        ...stats,
        sent: { ...stats.sent },
        queued: queue.length,
      };
    },
  };
}
