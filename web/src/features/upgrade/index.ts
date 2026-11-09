// `features/upgrade` (WP-CFEAT-24, WP-CFEAT-25): the safe-upgrade windows, the element windows, the awakening windows. See `upgradeModule.ts`.
export { upgradeModule, upgradeOf, type UpgradeFeature } from './upgradeModule';
export { createUpgradeSenders, type UpgradeSenders } from './senders';
export { createUpgradeCommon, freeOrSay, type UpgradeCommon } from './common';
export { AutoSmelt } from './safeSmelt/autoSmelt';
export { OneShotSmelt, PIERCING_FEE, SMELT_SLOTS } from './safeSmelt/oneShotSmelt';
export { createSmeltEnv, registerSafeSmelt, type SafeSmeltFeature } from './safeSmelt/module';
export type { SmeltEnv, SmeltMachine, SmeltSend } from './safeSmelt/types';
export { ELEMENT_REMOVED, registerElement, type ElementFeature } from './element/module';
export { stoneFits, stoneOf } from './element/stones';
export { AWAKE_KEEP_MS, registerAwaken, type AwakenFeature } from './awaken/module';
