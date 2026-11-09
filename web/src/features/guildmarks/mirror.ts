// The level and the job of OTHER players (WP-REND-26): the master / hero icon above a name reads `visual.level` and `visual.job`. The spawn fills them
// (`game/world/visual.ts: playerVisual`), `SetLevel` and `JobChanged` of another mover keep them current. The own values come from `selfStore`
// (its level-ups never reach `visual`); the values go with the entity record.

import type { FeatureContext } from '../common/module';

/** Registers the two handlers. */
export function registerMirror(fc: FeatureContext): void {
  const other = (objId: number) => {
    const rec = fc.entities.get(objId);
    return rec && !rec.removed && !rec.isSelf && rec.visual?.kind === 'player' ? rec : undefined;
  };
  fc.net.on('setLevel', (m, objId) => {
    const rec = other(objId);
    if (rec?.visual) rec.visual.level = m.level;
  });
  fc.net.on('jobChanged', (m, objId) => {
    const rec = other(objId);
    if (rec?.visual) rec.visual.job = m.job;
  });
}
