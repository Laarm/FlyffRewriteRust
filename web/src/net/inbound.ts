// Inbound frame queue (Plan/40 section 5.3, CA4): `ws.onmessage` only enqueues the RAW frame with its
// receipt time (O(1)); the queue is drained at step 1 of every loop iteration and of every background
// pump, in arrival order. Decoding and dispatch happen in the registry (`net/registry.ts`).

import { now as defaultNow } from '../platform/time';
import type { Unsubscribe } from '../platform/visibility';

export interface RawFrame {
  readonly bytes: Uint8Array;
  /** `performance.now()` when the frame was received (the clock estimator uses this, R4). */
  readonly recvAt: number;
  /** Monotonic arrival number of this queue. */
  readonly seq: number;
}

export type OverflowReason = 'frames' | 'bytes';

export interface InboundStats {
  /** Frames accepted by `push`. */
  readonly pushed: number;
  /** Frames handed to the dispatcher by `drain`. */
  readonly drained: number;
  /** Drains that took longer than `longDrainMs` (they still processed every frame, R2). */
  readonly longDrains: number;
  /** Duration of the last drain in ms. */
  readonly lastDrainMs: number;
  readonly maxDepth: number;
  /** Frames refused after an overflow. */
  readonly dropped: number;
  readonly overflows: number;
}

export interface InboundQueue {
  /** Appends a frame. After an overflow frames are refused until `clear()`. */
  push(frame: RawFrame): void;
  /** Convenience: builds the `RawFrame` (monotonic `seq`, `recvAt` defaults to the current time). */
  enqueue(bytes: Uint8Array, recvAt?: number): void;
  /**
   * Processes every frame queued at call time, in order (no partial batch, R2) and returns their count.
   * Frames pushed while draining wait for the next drain. If `dispatch` throws, the frames not yet
   * processed stay queued.
   */
  drain(dispatch: (frame: RawFrame) => void): number;
  /** Drops everything and re-opens an overflowed queue (a new session, after a resync). */
  clear(): void;
  /** Called after every accepted `push` (the background pump listens, section 10.5). */
  onPush(cb: () => void): Unsubscribe;
  readonly depth: number;
  readonly bytes: number;
  readonly overflowed: boolean;
  readonly stats: InboundStats;
}

export interface InboundQueueOptions {
  /** Back-pressure: more frames than this (default 2048, R3) request an L2 resync. */
  readonly maxFrames?: number;
  /** Back-pressure: more bytes than this (default 32 MiB, R3). */
  readonly maxBytes?: number;
  /** A drain slower than this counts in `stats.longDrains` (default 8 ms, R2). */
  readonly longDrainMs?: number;
  /** Time source (default `platform/time`). */
  readonly now?: () => number;
  /** Called once per overflow; the owner starts the L2 resync (section 12.2). */
  readonly onOverflow?: (reason: OverflowReason) => void;
}

export const MAX_INBOUND_FRAMES = 2048;
export const MAX_INBOUND_BYTES = 32 * 1024 * 1024;
export const LONG_DRAIN_MS = 8;

export function createInboundQueue(options: InboundQueueOptions = {}): InboundQueue {
  const maxFrames = options.maxFrames ?? MAX_INBOUND_FRAMES;
  const maxBytes = options.maxBytes ?? MAX_INBOUND_BYTES;
  const longDrainMs = options.longDrainMs ?? LONG_DRAIN_MS;
  const now = options.now ?? defaultNow;
  const pushListeners = new Set<() => void>();

  let items: (RawFrame | undefined)[] = [];
  let head = 0;
  let bytes = 0;
  let seq = 0;
  let overflowed = false;
  const stats = {
    pushed: 0,
    drained: 0,
    longDrains: 0,
    lastDrainMs: 0,
    maxDepth: 0,
    dropped: 0,
    overflows: 0,
  };

  const depth = (): number => items.length - head;

  const push = (frame: RawFrame): void => {
    if (overflowed) {
      stats.dropped += 1;
      return;
    }
    items.push(frame);
    bytes += frame.bytes.byteLength;
    stats.pushed += 1;
    const d = depth();
    if (d > stats.maxDepth) stats.maxDepth = d;
    if (d > maxFrames || bytes > maxBytes) {
      overflowed = true;
      stats.overflows += 1;
      options.onOverflow?.(d > maxFrames ? 'frames' : 'bytes');
    }
    for (const cb of [...pushListeners]) cb();
  };

  return {
    push,
    enqueue(frameBytes, recvAt) {
      seq += 1;
      push({ bytes: frameBytes, recvAt: recvAt ?? now(), seq });
    },
    drain(dispatch) {
      const end = items.length;
      if (head >= end) return 0;
      const started = now();
      let n = 0;
      try {
        while (head < end) {
          const frame = items[head];
          items[head] = undefined;
          head += 1;
          if (frame === undefined) continue;
          bytes -= frame.bytes.byteLength;
          n += 1;
          stats.drained += 1;
          dispatch(frame);
        }
      } finally {
        if (head >= items.length) {
          items = [];
          head = 0;
        } else if (head > 1024 && head * 2 > items.length) {
          items = items.slice(head);
          head = 0;
        }
        const took = now() - started;
        stats.lastDrainMs = took;
        if (took > longDrainMs) stats.longDrains += 1;
      }
      return n;
    },
    clear() {
      items = [];
      head = 0;
      bytes = 0;
      overflowed = false;
    },
    onPush(cb) {
      pushListeners.add(cb);
      return () => {
        pushListeners.delete(cb);
      };
    },
    get depth() {
      return depth();
    },
    get bytes() {
      return bytes;
    },
    get overflowed() {
      return overflowed;
    },
    stats,
  };
}
