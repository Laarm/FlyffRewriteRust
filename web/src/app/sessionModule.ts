// Core handler module `session` (Plan/40 section 1.3): the lobby messages that drive the screen state
// machine (phase Core). `HelloAck`/`Kick` are `net/connection.ts` (WP-CLIENT-12); `WorldReadInfo`, `Replace`
// and `SpawnSelf` are `game/world/*` (WP-CLIENT-19/-23); the content of `CharList`/`LobbyError` for the
// screens is 45's `lobbyStore` (a Feature handler).

import { Phase } from '../net/registry';
import type { ClientContext } from './context';
import type { HandlerModule } from './features';
import type { CharSlotLite } from './stateMachine';

export const sessionModule: HandlerModule = {
  id: 'session',
  register(ctx: ClientContext): void {
    const { registry } = ctx.net;
    registry.on('charList', Phase.Core, 'session', (_objId, list) => {
      const slots: CharSlotLite[] = list.slots.map((s) => ({
        slot: s.slot,
        idPlayer: s.idPlayer,
        name: s.name,
        blocked: s.blocked,
      }));
      ctx.machine.dispatch({ type: 'charList', slots });
    });
    registry.on('lobbyError', Phase.Core, 'session', (_objId, err) => {
      ctx.machine.dispatch({ type: 'lobbyError', code: err.code });
    });
  },
};
