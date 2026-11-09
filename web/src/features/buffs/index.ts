// `features/buffs` (WP-CFEAT-30): the buff maps, the buff icons of the world strip and the status window, the duration list. See
// `buffsModule.ts`.
export { BuffSet, buffKey, remainOf, type Buff, type BuffRules } from './buffSet';
export { buffsModule, buffsOf, type BuffsFeature, type CastState } from './buffsModule';
export {
  buildHud,
  buildStatus,
  buffIconPath,
  buffTimeGap,
  hudLimit,
  smRemaining,
  type DisplayEnv,
  type HudInput,
} from './display';
export { buildDurationRows, type DurationEnv } from './durations';
export { clockText, statusTimerBetter, statusTimerPlain, worldTimer } from './format';
export { EXP_FAMILIES, expFamilyOf } from './ids';
export { SM, SM_ITEM_ID, SM_MAX } from './sm';
export { expTooltip, hudTooltip, smTooltip, statusTooltip, type TooltipEnv } from './tooltips';
export * from './types';
