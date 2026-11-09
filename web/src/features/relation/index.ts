// `features/relation` (WP-CFEAT-40): the couple, campus, honor and cheer features - the messages and their texts, the couple window and its proposal boxes, the campus invitation and
// severance, the campus tab of the messenger, the honor tab of the character window and the title above the names, the cheer motion, the couple motion and the tooltip with the cheer
// points. The player-data cache that the windows read is `features/common/playerData.ts`.
//
//   relationOf(ctx)                      { store, senders, proposeTo(objId), cheer(objId), invite(objId), sever(characterId, name) }
//   store `relation` (scope session)     { couple, campus, campusPoints, honor: { counters, earned, selected }, cheer: { points, restUntil } }
//   probe                                __flyffDebug.features.relation(), relationPoints(), relationPropose(objId), relationCheer(objId), relationInvite(objId)
export { relationModule, relationOf } from './relationModule';
export type { RelationFeature } from './relationModule';
export {
  defineRelationStore,
  initialRelationState,
  type CampusMemberRecord,
  type CampusRecord,
  type CheerPoints,
  type CoupleRecord,
  type HonorState,
  type RelationState,
  type RelationStore,
} from './store';
export * from './reducers';
export {
  COUPLE_MAX_LEVEL,
  coupleLevel,
  coupleRate,
  coupleTotal,
  gaugeFill,
  gaugeRate,
  skillRowAt,
  type SkillLevelRow,
} from './coupleLevel';
export {
  applyHonorList,
  earnedFromCounters,
  type EarnedTitle,
  HonorTable,
  MAX_HONOR_TITLE,
} from './honorTitles';
export {
  cheerRestSeconds,
  cheerRestText,
  cheerTooltip,
  type CheerState,
  MAX_CHEERPOINT,
} from './cheer';
export { relationSenders, type RelationSenders } from './senders';
export {
  campusViewOf,
  coupleInfoView,
  coupleSkillView,
  honorView,
  markSprite,
  type CachedPlayer,
  type SkillFacts,
  type ViewFacts,
} from './view';
