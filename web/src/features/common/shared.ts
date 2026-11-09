// Per-context shared helpers of the features (Plan/45 §2.4/§2.5): the request gate and the transactional-window registry.
// One instance per `ClientContext` (tests build several contexts; there are no module-level singletons).

import type { ClientContext } from '../../app/context';
import { createRequestGate, type RequestGate } from './requestGate';
import { createTransactionalRegistry, type TransactionalRegistry } from './transactional';

export interface FeatureShared {
  readonly gate: RequestGate;
  readonly transactional: TransactionalRegistry;
}

const shared = new WeakMap<ClientContext, FeatureShared>();

export function sharedOf(ctx: ClientContext): FeatureShared {
  let s = shared.get(ctx);
  if (!s) {
    s = {
      gate: createRequestGate(),
      transactional: createTransactionalRegistry((id, error) => {
        ctx.log.error(
          'features',
          `closeTransactional ${id}: ${error instanceof Error ? error.message : String(error)}`,
        );
      }),
    };
    shared.set(ctx, s);
  }
  return s;
}
