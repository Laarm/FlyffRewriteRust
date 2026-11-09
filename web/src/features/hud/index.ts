// `web/src/features/hud`: the HUD messages of the world view (WP-CFEAT-07): centre client messages and captions.
export { Captions, CAPTION_HOLD_MS } from './captions';
export type { Caption } from './captions';
export { CentreMessages, CENTRE_FADE_STEP, CENTRE_HOLD_MS, CENTRE_MAX_LINES } from './centre';
export type { CentreLine } from './centre';
export { HudState, CAPTION_DIV_BIG, CAPTION_DIV_SMALL } from './hud';
export { HudOverlay } from './HudOverlay';
export { closeFieldWindows, domMeasure, hudModule, hudOf, mountHudOverlay } from './hudModule';
export { statusModule, statusOf, TARGET_PERIOD_MS } from './status/statusModule';
export type { StatusFeature } from './status/statusModule';
export { createStatusView, MODE_EXPUP_STOP, statusTitle, statusViewOf } from './status/view';
export {
  ELEMENT_NAMES,
  EFFECTIVE_ELEMENT_NAMES,
  gaugeFill,
  isHeroJob,
  isMasterJob,
  targetFrameModel,
  targetVisible,
} from './targetFrame/model';
export type { TargetEnv, TargetFrameModel, TargetInput } from './targetFrame/model';
