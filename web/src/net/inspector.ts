// `?debug=net` packet inspector (Plan/40 section 5.7, WP-PROTO-06): logs every decoded server message and
// every sent envelope as canonical protobuf JSON. Available in production builds; the session token in
// `Hello` is redacted. Entries go through the platform logger (ring buffer + console at info level).

import { toJson } from '@bufbuild/protobuf';
import { ServerMsgSchema, type ServerBatch } from '../gen/proto/flyff/v1/envelope_pb';
import type { Logger } from '../platform/log';
import type { ClientMsg } from './codec';
import { envelopeJson } from './sender';

export interface Inspector {
  /** Logs every message of a decoded batch. */
  onBatch(
    batch: ServerBatch,
    ctx: { readonly serverFrame: number; readonly batchSeq: number },
  ): void;
  /** Logs one sent envelope. */
  onSend(msg: ClientMsg): void;
  readonly logged: number;
}

/** `true` when the query string contains `debug=net` (alone or in a comma list such as `debug=net,perf`). */
export function netDebugEnabled(search: string): boolean {
  const params = new URLSearchParams(search);
  return params.getAll('debug').some((v) => v.split(',').some((flag) => flag.trim() === 'net'));
}

/** Cap of one log line, so a spawn batch with a full inventory does not flood the ring buffer. */
const MAX_LINE = 4000;

function clip(text: string): string {
  return text.length > MAX_LINE
    ? `${text.slice(0, MAX_LINE)}...(${String(text.length)} chars)`
    : text;
}

export function createInspector(log: Pick<Logger, 'info'>): Inspector {
  let logged = 0;
  return {
    onBatch(batch, ctx) {
      for (const m of batch.msgs) {
        logged += 1;
        const json = toJson(ServerMsgSchema, m);
        log.info(
          'net.rx',
          clip(`#${String(ctx.batchSeq)} f${String(ctx.serverFrame)} ${JSON.stringify(json)}`),
        );
      }
    },
    onSend(msg) {
      logged += 1;
      log.info('net.tx', clip(JSON.stringify(envelopeJson(msg))));
    },
    get logged() {
      return logged;
    },
  };
}
