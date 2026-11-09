// Protocol codec (Plan/11 section 3.7): the wire format only; session semantics (Hello/HelloAck,
// Kick, clock cadence) belong to net/connection.ts (WP-CLIENT-12).

import { create, fromBinary, toBinary } from '@bufbuild/protobuf';
import {
  ClientEnvelopeSchema,
  ServerBatchSchema,
  ServerMsgSchema,
  type ClientEnvelope,
  type ServerBatch,
  type ServerMsg,
} from '../gen/proto/flyff/v1/envelope_pb';
import { n64 } from '../gen/proto/js_safe';

/** One client-to-server message as the `msg` oneof of `ClientEnvelope`. */
export type ClientMsg = ClientEnvelope['msg'];

/** Encodes one `ClientEnvelope` (one WebSocket binary message). */
export function encodeClient(msg: ClientMsg): Uint8Array {
  return toBinary(ClientEnvelopeSchema, create(ClientEnvelopeSchema, { msg }));
}

/** Decodes one `ServerBatch` frame. Unknown messages decode with an unset oneof (skippable). */
export function decodeBatch(bytes: Uint8Array): ServerBatch {
  return fromBinary(ServerBatchSchema, bytes);
}

/** The oneof case names of `ServerMsg.msg` (protobuf-es `localName`, e.g. `moverStateNtf`). */
export type ServerCase = NonNullable<ServerMsg['msg']['case']>;

/** The message type of one `ServerMsg` case. */
export type ServerValue<K extends ServerCase> = Extract<ServerMsg['msg'], { case: K }>['value'];

type Case = ServerCase;

/** Every case this build knows, from the descriptor (a case outside this set is a tag of a newer server). */
export const SERVER_CASES: ReadonlySet<string> = new Set(
  ServerMsgSchema.oneofs.flatMap((o) => o.fields.map((f) => f.localName)),
);

/** Handlers by oneof case; `frame` is `n64(ServerBatch.server_frame)` (Plan/11 section 4.3). */
export type ServerHandlers = {
  [K in Case]?: (objId: number, value: ServerValue<K>, frame: number) => void;
};

/**
 * Decodes a batch and calls the handler of every message in order. A message whose case is unknown to
 * this build (or has no handler) is reported through `onUnknown` and skipped; the rest of the batch is
 * unaffected (Plan/11 section 3.7).
 */
export function dispatchBatch(
  bytes: Uint8Array,
  handlers: ServerHandlers,
  onUnknown: (tag: string) => void,
): void {
  dispatchDecoded(decodeBatch(bytes), handlers, onUnknown);
}

/** {@link dispatchBatch} for an already decoded batch (the handler registry decodes first to read the frame). */
export function dispatchDecoded(
  batch: ServerBatch,
  handlers: ServerHandlers,
  onUnknown: (tag: string) => void,
): void {
  const frame = n64(batch.serverFrame);
  for (const m of batch.msgs) {
    callHandler(handlers, m, frame, onUnknown);
  }
}

function callHandler(
  handlers: ServerHandlers,
  m: ServerMsg,
  frame: number,
  onUnknown: (tag: string) => void,
): void {
  const msg = m.msg;
  if (msg.case === undefined) {
    const numbers = (m.$unknown ?? []).map((f) => f.no);
    onUnknown(numbers.length > 0 ? `tag:${numbers.join(',')}` : 'empty');
    return;
  }
  // `handlers[msg.case]` and `msg.value` are correlated; TypeScript cannot express that for unions.
  const handler = handlers[msg.case] as ((o: number, v: unknown, f: number) => void) | undefined;
  if (handler === undefined) {
    onUnknown(msg.case);
    return;
  }
  handler(m.objId, msg.value, frame);
}
