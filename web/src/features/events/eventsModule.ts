// The `events` feature module (Plan/45 §28.0; WP-CFEAT-07, WP-CFEAT-41): the event flags (`FlyffEvent`, `SetLocalEvent`) and the one HUD timer line. The M7 event tasks register their
// own modules under the same feature id and write their slices of `eventStore`.
//
//   FlyffEvent, SetLocalEvent     `flags`: the whole array of the join batch, one byte of a change; `eventOf( ctx ).isEventOn( id )` is `flags[ id ] != 0`
//   hudTimer                      `setHudTimer( label, ms, src )`, `clearHudTimer( src? )`: the line under the top of the world (`features/events/siege/SiegeHud.tsx` draws it)

import type { FeatureContext, FeatureModule } from '../common/module';
import { addProbeMember } from '../common/probe';
import { type EventFeature, eventOf, setEventOf } from './api';
import { replaceFlags, setFlag, flagOn } from './flags';
import { defineEventStore, type HudTimerSource } from './store';

export { eventOf } from './api';

export const eventsModule: FeatureModule = {
  id: 'events',
  milestone: 2,
  register(fc) {
    registerEvents(fc);
  },
};

/** Registers the handlers of the flags once per context (the siege module needs the events and asks for them when the order of the modules put it first). */
export function registerEvents(fc: FeatureContext): EventFeature {
  const ctx = fc.app;
  const known = eventOf(ctx);
  if (known) return known;
  const store = defineEventStore(ctx.stores.registry);

  fc.net.on('flyffEvent', (m) => {
    store.setState({ flags: replaceFlags(m.flags) });
  });
  fc.net.on('setLocalEvent', (m) => {
    const flags = store.getState().flags;
    const next = setFlag(flags, m.eventId, m.state);
    if (next !== flags) store.setState({ flags: next });
  });

  const feature: EventFeature = {
    store,
    isEventOn: (id) => flagOn(store.getState().flags, id),
    setHudTimer(label, ms, src, waitSec) {
      store.setState({
        hudTimer: {
          label,
          endMs: ctx.now() + ms,
          src,
          ...(waitSec === true ? { waitSec: true } : {}),
        },
      });
    },
    clearHudTimer(src?: HudTimerSource) {
      const t = store.getState().hudTimer;
      if (t === null || (src !== undefined && t.src !== src)) return;
      store.setState({ hudTimer: null });
    },
  };
  setEventOf(ctx, feature);
  addProbeMember(ctx, 'events', () => {
    const s = store.getState();
    const on: number[] = [];
    s.flags.forEach((v, i) => {
      if (v !== 0) on.push(i);
    });
    return { on, hudTimer: s.hudTimer };
  });
  return feature;
}
