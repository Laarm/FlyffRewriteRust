// The `upgrade` feature module (Plan/45 §18; WP-CFEAT-24, WP-CFEAT-25): the windows of the safe upgrade (auto and one-shot), the element removal and commercial
// stones, the awakening, the protected awakening choice, the blessing / level-down / pet awakening cancel, over `ui.upgrade` (WP-UI-29, -30, -31) and the NPC menu
// table of the `npc` feature. The random-option retry scroll is wired by the `itemuse` feature (`features/upgrade/randomOption` documents and tests it). The ultimate,
// jewel, piercing, item-transy and exchange windows are WP-CFEAT-26's.

import type { ClientContext } from '../../app/context';
import type { FeatureModule } from '../common/module';
import { addProbeMember } from '../common/probe';
import { inventoryOf } from '../inventory';
import { menuOf } from '../npc';
import { type AwakenFeature, registerAwaken } from './awaken/module';
import { createUpgradeCommon, type UpgradeCommon } from './common';
import { type ElementFeature, registerElement } from './element/module';
import { type SafeSmeltFeature, registerSafeSmelt } from './safeSmelt/module';
import { createUpgradeSenders, type UpgradeSenders } from './senders';

export interface UpgradeFeature {
  readonly senders: UpgradeSenders;
  readonly common: UpgradeCommon;
  readonly safeSmelt: SafeSmeltFeature;
  readonly element: ElementFeature;
  readonly awaken: AwakenFeature;
}

const features = new WeakMap<ClientContext, UpgradeFeature>();

/** The upgrade feature of a context; `undefined` before the module registered. */
export function upgradeOf(ctx: ClientContext): UpgradeFeature | undefined {
  return features.get(ctx);
}

export const upgradeModule: FeatureModule = {
  id: 'upgrade',
  milestone: 4,
  register(fc) {
    const ctx = fc.app;
    const inv = inventoryOf(ctx);
    const menu = menuOf(ctx);
    // both belong to modules that register earlier (`inventory`, `npc`); without them there is nothing to wire
    if (!inv || !menu) return;
    const common = createUpgradeCommon(fc, inv);
    const senders = createUpgradeSenders(ctx);
    const safeSmelt = registerSafeSmelt(common, menu, senders);
    const element = registerElement(common, menu, senders);
    const awaken = registerAwaken(common, menu, senders);
    const feature: UpgradeFeature = { senders, common, safeSmelt, element, awaken };
    features.set(ctx, feature);
    addProbeMember(ctx, 'upgrade', () => {
      const m = safeSmelt.machine();
      return {
        safe: {
          ...safeSmelt.stats(),
          kind: m?.kind ?? null,
          running: m?.running ?? false,
          counters: m?.counters ?? null,
          view: m?.view() ?? null,
        },
        element: { ...element.stats(), pending: element.removalPending() },
        awaken: { ...awaken.stats(), choice: awaken.pendingChoice() !== null },
      };
    });
  },
};
