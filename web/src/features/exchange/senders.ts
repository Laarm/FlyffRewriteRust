// The client -> server send of the exchange window (Plan/45 §11.2, §18.6; WP-CFEAT-26; rule C9: the send site carries its `// legacy-send:` marker).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { ExchangeReqSchema } from '../../gen/proto/flyff/v1/upgrade_pb';

export interface ExchangeSenders {
  /** `SendExchange( mmi, index )`: run row `index` of the exchange list of NPC menu `mmi`. */
  exchange(mmi: number, setIndex: number): void;
}

export function createExchangeSenders(ctx: ClientContext): ExchangeSenders {
  return {
    exchange(mmiId, setIndex) {
      // legacy-send: SendExchange
      ctx.net.sender.send({
        case: 'exchangeReq',
        value: create(ExchangeReqSchema, { mmiId, setIndex }),
      });
    },
  };
}
