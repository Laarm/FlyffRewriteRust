// Protocol client (Plan/40 section 5, Plan/11 section 3.7). WP-PROTO-06: codec, clock, socket, inspector,
// int64 helpers; WP-CLIENT-06: inbound queue, handler registry, sender; `connection.ts` (Hello/HelloAck,
// Kick, reconnect, liveness: WP-CLIENT-12/-24) follows.
export {
  decodeBatch,
  dispatchBatch,
  dispatchDecoded,
  encodeClient,
  SERVER_CASES,
  type ClientMsg,
  type ServerCase,
  type ServerHandlers,
  type ServerValue,
} from './codec';
export { ClockEstimator, CLOCK_WINDOW, type ClockSample } from './clock';
export {
  CloseCode,
  helloAckOutcome,
  kickCloseCode,
  kickOutcome,
  type SessionOutcome,
} from './closeCodes';
export { createNetClient, type NetClient, type NetClientOptions } from './client';
export {
  createInboundQueue,
  LONG_DRAIN_MS,
  MAX_INBOUND_BYTES,
  MAX_INBOUND_FRAMES,
  type InboundQueue,
  type InboundQueueOptions,
  type InboundStats,
  type OverflowReason,
  type RawFrame,
} from './inbound';
export { createInspector, netDebugEnabled, type Inspector } from './inspector';
export { b64, isJsSafeField, JS_SAFE_FIELDS, n64, n64or0 } from './int64';
export {
  createHandlerRegistry,
  Phase,
  SPAWN_CASES,
  type BatchHooks,
  type Case,
  type HandlerRegistry,
  type HandlerRegistryOptions,
  type MsgCtx,
  type MsgHandler,
  type ObserverGuard,
  type RegistryStats,
  type Value,
} from './registry';
export {
  createNetSender,
  DEFAULT_SENDER_LIMITS,
  envelopeJson,
  MAX_QUEUED,
  type NetSender,
  type NetSenderOptions,
  type SenderLimits,
  type SenderStats,
  type SenderTransport,
  type SendOptions,
  type SendResult,
} from './sender';
export { openSocket, type Transport, type TransportStats } from './socket';
