// The `pvp` feature module, resurrection part (Plan/45 §25.4, 17 §9.4, §11.1; `src:Neuz/DPClient.cpp:3335-3360`, `WndField.cpp:13307-13370`;
// WP-CFEAT-36): another player's Resurrection skill offers to revive the dead player. `ResurrectionMessage` opens `APP_RESURRECTION` (only when
// none is open); Yes sends `ResurrectionOk`, No `ResurrectionCancel`. The revival window sends `ResurrectionCancel` before its own request when
// the offer is still open (`revivalModule`); the offer is void once the player lives again, so the window goes with the revival messages (the
// original leaves it open until the player answers: the answer would be ignored by the server).

import type { ClientContext } from '../../../app/context';
import { combatRuntime } from '../../../game/combat';
import { type FeatureContext, type FeatureModule, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { sendResurrectionCancel } from '../revival/senders';
import { sendResurrectionOk } from './senders';

/** The public face of the resurrection part of a context. */
export interface ResurrectionFeature {
  /** The confirm window is open. */
  isOpen(): boolean;
  /** `OnResurrectionMessage`: opens the window unless it is open already (a test and the e2e call it, the message handler too). */
  offer(): boolean;
}

const features = new WeakMap<ClientContext, ResurrectionFeature>();
export function resurrectionOf(ctx: ClientContext): ResurrectionFeature | undefined {
  return features.get(ctx);
}

export const resurrectionModule: FeatureModule = {
  id: 'pvp',
  milestone: 5,
  register(fc) {
    registerResurrection(fc);
  },
};

function registerResurrection(fc: FeatureContext): ResurrectionFeature {
  const ctx = fc.app;
  const isOpen = (): boolean => fullUi(ctx)?.duel.isResurrectionOpen() ?? false;
  const offer = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    return (
      ui.duel.openResurrection({
        yes: () => {
          sendResurrectionOk(ctx);
        },
        no: () => {
          sendResurrectionCancel(ctx);
        },
      }) !== null
    );
  };
  fc.net.on('resurrectionMessage', () => {
    offer();
  });

  // the offer ends with the revival of the own character (the server cleared it, `ResurrectionOk` would be ignored)
  const revived = (): void => {
    fullUi(ctx)?.duel.closeResurrection();
  };
  combatRuntime(ctx).onOwnRevived(revived);
  for (const c of ['revivalNtf', 'revivalToLodestarNtf', 'resurrection'] as const) {
    fc.net.on(c, (_m, objId) => {
      if (objId === 0 || ctx.entities.get(objId)?.isSelf === true) revived();
    });
  }
  // a logout or a world change ends the offer too
  fc.transactional.register({
    id: 'pvp.resurrection',
    reasons: new Set(['WorldChange', 'Logout', 'Kicked']),
    close: revived,
  });

  const feature: ResurrectionFeature = { isOpen, offer };
  features.set(ctx, feature);
  addProbeMember(ctx, 'resurrection', () => ({ open: isOpen() }));
  return feature;
}
