// `features/guild` (WP-CFEAT-37, WP-CFEAT-38): the guild feature - the messages and their texts, the guild window with its pages, the invitation, naming, notice, logo, disband, salary and
// contribution dialogs, the member menu, the guild bank with its log and the cloak, the guild-war dialogs, the visuals of the movers and the player-data queries of the members.
export { guildModule } from './guildModule';
export { guildOf } from './api';
export type { GuildEnvSeams, GuildFeature } from './api';
export {
  defineGuildStore,
  initialGuildState,
  ownBrief,
  isMasterOf,
  rankOf,
  rankCount,
} from './store';
export type {
  BankLogs,
  GuildBankState,
  GuildState,
  GuildStore,
  MemberRecord,
  OwnGuild,
  WarSide,
  WarState,
} from './store';
export * from './reducers';
export { guildSenders } from './senders';
export type { GuildSenders } from './senders';
export { APP_MASK, GH_MASK, matrixOf, PowerDraft, POWER_BITS, wordsOf, withBit } from './power';
export type { PowerMatrix } from './power';
export { canContribute, gemPoints, meritResultTid } from './contribution';
export type { ContributionResult, GemStack } from './contribution';
export {
  checkGuildName,
  checkNickname,
  checkSalary,
  nameProblem,
  noticeFits,
  warTargetOk,
} from './names';
export { formatLog, formatLogLine, logDate } from './bankLog';
export type { LogEntry, LogEnv } from './bankLog';
export {
  GUILD_LOGO_COUNT,
  GUILD_LOGO_GM_FROM,
  GUILD_LOGO_SIZE,
  guildLogoPath,
  guildLogoSize,
  isPickableLogo,
} from './logos';
export { buildGuildView, buildPowerView, memberRows, maxMembers, RANK_CAPS } from './view';
