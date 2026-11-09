// Thin WebSocket transport (Plan/11 section 3.7, WP-PROTO-06): connect/send/close and binary frames only.
// No protocol knowledge: Hello/HelloAck, Kick, close-code policy, the clock cadence, liveness and the
// reconnect loop belong to `net/connection.ts` (WP-CLIENT-12/-24), which wraps this module.

import type { Unsubscribe } from '../platform/visibility';

export interface TransportStats {
  /** Text frames received (they are ignored: the protocol is binary only, Plan/11 section 1.2). */
  readonly textFrames: number;
  readonly binaryFrames: number;
  readonly bytesIn: number;
  readonly bytesOut: number;
  /** `send` calls while the socket was not open (the bytes were not written). */
  readonly unsent: number;
  /** Frames of an unsupported type (Blob), ignored. */
  readonly ignoredFrames: number;
  /** `error` events (always followed by `close`). */
  readonly errors: number;
}

export interface Transport {
  /** Writes one binary WebSocket message; ignored (and counted in `stats.unsent`) unless open. */
  send(bytes: Uint8Array): void;
  /** Starts the closing handshake; the `onClose` callbacks fire when it is complete. */
  close(code: number, reason?: string): void;
  readonly bufferedAmount: number;
  /** `true` while `readyState === OPEN`. */
  readonly isOpen: boolean;
  readonly stats: TransportStats;
  onOpen(cb: () => void): Unsubscribe;
  /** Binary frames only, in arrival order. */
  onMessage(cb: (bytes: Uint8Array) => void): Unsubscribe;
  onClose(cb: (code: number, reason: string, wasClean: boolean) => void): Unsubscribe;
}

const OPEN = 1;

function isArrayBuffer(v: unknown): v is ArrayBuffer {
  return Object.prototype.toString.call(v) === '[object ArrayBuffer]';
}

/**
 * Opens a WebSocket to `url` with `binaryType = "arraybuffer"`. `ctor` is the test seam (defaults to the
 * global `WebSocket`). A constructor failure (bad URL, blocked by CSP) is reported as a close with code
 * 1006 on the next microtask, so callers handle one failure path.
 */
export function openSocket(url: string, ctor: typeof WebSocket = WebSocket): Transport {
  const openCbs = new Set<() => void>();
  const messageCbs = new Set<(bytes: Uint8Array) => void>();
  const closeCbs = new Set<(code: number, reason: string, wasClean: boolean) => void>();
  const stats = {
    textFrames: 0,
    binaryFrames: 0,
    bytesIn: 0,
    bytesOut: 0,
    unsent: 0,
    ignoredFrames: 0,
    errors: 0,
  };
  let closed = false;

  const fireClose = (code: number, reason: string, wasClean: boolean): void => {
    if (closed) return;
    closed = true;
    for (const cb of [...closeCbs]) cb(code, reason, wasClean);
  };

  let ws: WebSocket | null = null;
  try {
    ws = new ctor(url);
    ws.binaryType = 'arraybuffer';
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    queueMicrotask(() => {
      fireClose(1006, `socket creation failed: ${reason}`, false);
    });
  }

  if (ws) {
    ws.onopen = () => {
      for (const cb of [...openCbs]) cb();
    };
    ws.onmessage = (ev: MessageEvent) => {
      const data: unknown = ev.data;
      if (typeof data === 'string') {
        stats.textFrames += 1;
        return;
      }
      let bytes: Uint8Array;
      if (isArrayBuffer(data)) {
        bytes = new Uint8Array(data);
      } else if (ArrayBuffer.isView(data)) {
        bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      } else {
        stats.ignoredFrames += 1;
        return;
      }
      stats.binaryFrames += 1;
      stats.bytesIn += bytes.byteLength;
      for (const cb of [...messageCbs]) cb(bytes);
    };
    ws.onerror = () => {
      stats.errors += 1;
    };
    ws.onclose = (ev: CloseEvent) => {
      fireClose(ev.code, ev.reason, ev.wasClean);
    };
  }

  return {
    send(bytes) {
      if (ws?.readyState !== OPEN) {
        stats.unsent += 1;
        return;
      }
      stats.bytesOut += bytes.byteLength;
      // never SharedArrayBuffer-backed (banned on the plain-HTTP critical path, Plan/02 section 11.7)
      ws.send(bytes as Uint8Array<ArrayBuffer>);
    },
    close(code, reason) {
      if (!ws) return;
      try {
        ws.close(code, reason);
      } catch {
        // an invalid code (outside 1000/3000-4999) or an over-long reason: close without them
        try {
          ws.close();
        } catch {
          // already closed
        }
      }
    },
    get bufferedAmount() {
      return ws?.bufferedAmount ?? 0;
    },
    get isOpen() {
      return ws?.readyState === OPEN;
    },
    stats,
    onOpen(cb) {
      openCbs.add(cb);
      return () => {
        openCbs.delete(cb);
      };
    },
    onMessage(cb) {
      messageCbs.add(cb);
      return () => {
        messageCbs.delete(cb);
      };
    },
    onClose(cb) {
      closeCbs.add(cb);
      return () => {
        closeCbs.delete(cb);
      };
    },
  };
}
