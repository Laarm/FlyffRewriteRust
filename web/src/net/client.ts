// NetClient: the pieces of the protocol client wired together (Plan/40 section 2.3 `ctx.net`): the inbound
// queue, the handler registry, the sender and the clock estimator. `drain()` is step 1 of every loop
// iteration (section 10.2). The connection lifecycle (Hello/HelloAck/Kick, reconnect, liveness) is
// `net/connection.ts` (WP-CLIENT-12/-24) and attaches through `attach`/`detach`.

import type { Logger } from '../platform/log';
import { now as defaultNow } from '../platform/time';
import type { Unsubscribe } from '../platform/visibility';
import { ClockEstimator } from './clock';
import {
  createHandlerRegistry,
  type HandlerRegistry,
  type HandlerRegistryOptions,
} from './registry';
import { createInboundQueue, type InboundQueue, type InboundQueueOptions } from './inbound';
import { createNetSender, type NetSender, type NetSenderOptions } from './sender';
import type { Transport } from './socket';

export interface NetClient {
  readonly inbound: InboundQueue;
  readonly registry: HandlerRegistry;
  readonly sender: NetSender;
  readonly clock: ClockEstimator;
  /** The attached transport, if any. */
  readonly transport: Transport | null;
  /**
   * Loop step 1: dispatches every queued frame through the registry, sweeps the entity table (G5), then
   * runs the registry's drain-end hooks. Returns the number of frames processed.
   */
  drain(): number;
  /**
   * Connects a transport: inbound frames are queued with their receipt time, the sender writes to it.
   * Detaches a previous transport first and drops the frames of the previous session that were not drained
   * yet (the new session starts with its own join batch). Returns the detach function.
   */
  attach(transport: Transport): Unsubscribe;
  /** Disconnects the transport (the socket itself is closed by the connection module) and drops queued sends. */
  detach(): void;
}

export interface NetClientOptions {
  readonly log?: Pick<Logger, 'info' | 'warn' | 'error'>;
  readonly now?: () => number;
  readonly inbound?: InboundQueueOptions;
  readonly registry?: HandlerRegistryOptions;
  readonly sender?: Omit<NetSenderOptions, 'transport' | 'now'>;
  /** Called after dispatch and before the drain-end hooks (the entity sweep, G5). */
  readonly afterDispatch?: () => void;
}

export function createNetClient(options: NetClientOptions = {}): NetClient {
  const now = options.now ?? defaultNow;
  let transport: Transport | null = null;
  let unsubscribe: Unsubscribe | null = null;

  const inbound = createInboundQueue({ now, ...options.inbound });
  const registry = createHandlerRegistry({
    now,
    ...(options.log ? { log: options.log } : {}),
    ...options.registry,
  });
  const sender = createNetSender({
    now,
    ...(options.log ? { log: options.log } : {}),
    ...options.sender,
    transport: () => transport,
  });
  const clock = new ClockEstimator();
  const dispatch = (frame: Parameters<HandlerRegistry['dispatchFrame']>[0]): void => {
    registry.dispatchFrame(frame);
  };

  const detach = (): void => {
    unsubscribe?.();
    unsubscribe = null;
    transport = null;
    sender.clear();
  };

  return {
    inbound,
    registry,
    sender,
    clock,
    get transport() {
      return transport;
    },
    drain() {
      const n = inbound.drain(dispatch);
      if (n > 0) {
        options.afterDispatch?.();
        registry.endDrain();
      }
      return n;
    },
    attach(t) {
      detach();
      inbound.clear();
      transport = t;
      const offMessage = t.onMessage((bytes) => {
        inbound.enqueue(bytes, now());
      });
      unsubscribe = offMessage;
      return detach;
    },
    detach,
  };
}
