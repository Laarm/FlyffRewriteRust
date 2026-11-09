// The `guildmarks` feature (WP-REND-26, WP-INPUT-16; milestone 6): the client facts behind the guild marks above the players and the siege-map click rules.
//
//   modes     the `GCWAR_NOT_CLICK` / `GCWAR_RENDER_SKIP` bits of every player on the siege map, recomputed at the end of every drain (so a `ModifyMode` or a
//             late spawn of the same drain cannot leave a stale word) and again on `Replace` (`modes.ts`)
//   mirror    the level and job of other players for the master / hero icon (`mirror.ts`)
//   siege     `siegeMarksOf( ctx )`: the facts of ui-siege's slice that the overlay reads (`siegeView.ts`)
//
// Probe `__flyffDebug.features.guildmarks()`: the world, the viewer's roster status and the movers that carry the bits.

import type { ClientContext } from '../../app/context';
import { EntityKind } from '../../game/entities';
import type { FeatureModule } from '../common/module';
import { addProbeMember } from '../common/probe';
import type { EventStore } from '../events/store';
import { initialSiege } from '../events/store';
import { GCWAR_MODE_BITS, applyGcwarModes, gcStatusOf } from './modes';
import { registerMirror } from './mirror';
import { siegeMarksOf } from './siegeView';

const OWNER = 'guildmarks';

/** The roster of the siege slice, empty while the events store does not exist. */
function rosterOf(ctx: ClientContext) {
  const store = ctx.stores.registry.get('events') as unknown as EventStore | undefined;
  return store?.getState().siege.roster ?? initialSiege().roster;
}

export const guildMarksModule: FeatureModule = {
  id: 'guildmarks',
  milestone: 6,
  register(fc) {
    const ctx = fc.app;
    registerMirror(fc);
    const apply = (): void => {
      applyGcwarModes(
        fc.entities,
        ctx.stores.worldStore.getState().worldId,
        rosterOf(ctx),
        ctx.stores.selfStore.getState().idPlayer,
      );
    };
    ctx.net.registry.onDrainEnd(OWNER, apply);
    // a world change: the new world's rule (the entity table is rebuilt by the spawns that follow, whose drain ends in `apply` again)
    fc.net.on('replace', apply);
    addProbeMember(ctx, 'guildmarks', () => {
      const roster = rosterOf(ctx);
      const flagged: number[] = [];
      const players: { objId: number; idPlayer: number; hp: number; flagged: boolean }[] = [];
      fc.entities.forEach(EntityKind.Player, (rec) => {
        const hidden = ((rec.visual?.mode ?? 0) & GCWAR_MODE_BITS) !== 0;
        if (hidden) flagged.push(rec.objId);
        players.push({
          objId: rec.objId,
          idPlayer: rec.idPlayer ?? 0,
          hp: rec.hp,
          flagged: hidden,
        });
      });
      return {
        worldId: ctx.stores.worldStore.getState().worldId,
        viewerStatus: gcStatusOf(roster, ctx.stores.selfStore.getState().idPlayer),
        flagged: flagged.sort((a, b) => a - b),
        /** Every player in view: the object id, the character id, the HP of the entity record (0 after `MoverDeath`) and the GCWAR bits. */
        players: players.sort((a, b) => a.objId - b.objId),
        siege: siegeMarksOf(ctx),
      };
    });
  },
};
