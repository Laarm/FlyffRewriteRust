// World entry and world change (Plan/40 section 4.2, 9.8; WP-CLIENT-23; ref/09a section 6.3): `WorldReadInfo` (the join batch,
// re-attach) and `Replace` (teleport to another world or layer) put the state machine into `Loading` BEFORE the later messages
// of the same frame are dispatched (G7: the dispatch is in phase Core and the machine runs synchronously), reset the `map` scope
// (entity table, per-map stores, asset generation) and start the new world: `SimBridge.setWorld` (an immediate `set_world`),
// the renderer's `setWorld`, the collision streamer at the join position and the loading gate. The own same-world `SetPos` is
// `movement.ts` (no loading state). The loading image and the fade are 45's.

import type { ClientContext } from '../../app/context';
import { Phase } from '../../net/registry';
import type { WorldRuntime } from './runtime';

const OWNER = 'world.replace';

export function registerReplaceHandlers(ctx: ClientContext, rt: WorldRuntime): void {
  const { registry } = ctx.net;

  registry.on('worldReadInfo', Phase.Core, OWNER, (_objId, m) => {
    const t = ctx.machine.dispatch({ type: 'worldReadInfo', worldId: m.worldId, now: ctx.now() });
    // a WorldReadInfo outside Entering/Loading (a late frame after a quit) changes nothing
    if (t.status !== 'ok') return;
    rt.beginWorld(m.worldId, m.pos);
  });

  registry.on('replace', Phase.Core, OWNER, (_objId, m) => {
    const t = ctx.machine.dispatch({ type: 'replace', worldId: m.worldId, now: ctx.now() });
    if (t.status !== 'ok') return;
    rt.beginWorld(m.worldId, m.pos);
  });
}
