// Core handler module `connection` (Plan/40 section 1.3, 5.1, WP-CLIENT-12): the session messages of the WebSocket
// lifecycle. `HelloAck` and `Kick` become state-machine events (`net/connection.ts` decides with the 12 section 16.4
// table in `net/closeCodes.ts`), `ServerTime` feeds the clock estimator with the receipt time of the frame (R4),
// `AssetsPreparing` feeds the preparing-assets tracker (`assets/prepare.ts`, Plan/40 section 9.9).

import { Phase } from '../net/registry';
import type { ClientContext } from './context';
import type { HandlerModule } from './features';

export const connectionModule: HandlerModule = {
  id: 'connection',
  register(ctx: ClientContext): void {
    const { registry } = ctx.net;
    registry.on('helloAck', Phase.Core, 'connection', (_objId, ack, msg) => {
      ctx.connection.handleHelloAck(ack, msg);
    });
    registry.on('kick', Phase.Core, 'connection', (_objId, kick) => {
      ctx.connection.handleKick(kick);
    });
    registry.on('serverTime', Phase.Core, 'connection', (_objId, time, msg) => {
      ctx.connection.handleServerTime(time, msg);
    });
    registry.on('assetsPreparing', Phase.Core, 'connection', (_objId, progress) => {
      ctx.assetLayer.prepare.onPush(progress);
    });
    registry.onDrainEnd('connection', () => {
      ctx.connection.onDrainEnd();
    });
  },
};
