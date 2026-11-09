// `features/tooltip` (WP-CFEAT-16/17): the item tooltip composer and its module. See `compose.ts` for the section order.
export { colorsOf, FIRST_COLORS, ITEM_COLOR_SETS, SECOND_COLORS, THIRD_COLORS } from './colors';
export type { ItemTextColors } from './colors';
export { composeItem, createItemComposer, ULTIMATE_ICON } from './compose';
export {
  attackSpeedPercent,
  dstAwakeLine,
  dstPlusLine,
  dstSetLine,
  dstSignedLine,
  findDstString,
  isDstRate,
  putDestParam,
  signed,
  statAllUpLines,
} from './dst';
export { addSkillIndex, createComposeEnv } from './env';
export type { ComposeEnvDeps } from './env';
export { isInteriorityJob, jobTypeByLayout } from './jobs';
export { isTooltipItemView } from './model';
export type {
  AddSkillRow,
  ComposeEnv,
  Features,
  ItemTooltipExtra,
  PetRecord,
  ShopTooltipContext,
  TooltipItemRecord,
  TooltipItemView,
  TooltipOwnerInfo,
} from './model';
export { hostNumbers, tooltipModule, tooltipOf } from './module';
export type { ShopTooltipSource, TooltipFeature } from './module';
export { sprintf } from './printf';
export { RichBuilder, timeSpan } from './rich';
