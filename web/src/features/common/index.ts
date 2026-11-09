// `web/src/features/common/` (Plan/45 §2.4, WP-CFEAT-01): the module contract and the helpers every feature uses.
export { createFeatureContext, fullUi, toHandlerModule } from './module';
export type { FeatureContext, FeatureId, FeatureModule } from './module';
export { fmtCompact, fmtNumberComma, fmtSeparator } from './numbers';
export { formatTemplate, formatTid, SYSTEM_CHAT, sysChat } from './text';
export type { ChatPort } from './text';
export { createRequestGate } from './requestGate';
export type { RequestGate } from './requestGate';
export { createTransactionalRegistry } from './transactional';
export type { TransactionalReason, TransactionalRegistry, TransactionalRow } from './transactional';
export {
  feature,
  filterReservedApplets,
  filterReservedHotkeys,
  isReservedApplet,
  isReservedAppletHotkey,
  RESERVED_APPLET_HOTKEYS,
  RESERVED_APPLETS,
} from './featureFlags';
export type { ClientFeatureConfig, FeatureKey } from './featureFlags';
export { detectLobbyRules, nameRulesBytes } from './lobbyRules';
export type { LobbyRules, NameCheck } from './lobbyRules';
export { addProbeMember, featureProbe } from './probe';
export { sharedOf } from './shared';
export { confirmTid } from './confirm';
