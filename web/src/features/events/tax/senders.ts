// The send of the tax rate window (rule C9: `// legacy-send: <SendFn>`): `SendTaxRate`. The session identifies the sender (VR-001).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { TaxSetTaxRateSchema } from '../../../gen/proto/flyff/v1/lord_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders of the tax over one client context. */
export interface TaxSenders {
  /** `SendTaxRate( nCont, nSalesRate, nPurchaseRate )`: the master of the winning guild sets the rates of a continent. */
  taxRate(continent: number, salesRate: number, purchaseRate: number): void;
}

export function taxSenders(send: Send): TaxSenders {
  return {
    taxRate(continent, salesRate, purchaseRate) {
      // legacy-send: SendTaxRate
      send({
        case: 'taxSetTaxRate',
        value: create(TaxSetTaxRateSchema, {
          continent,
          salesRate,
          purchaseRate,
        }),
      });
    },
  };
}
