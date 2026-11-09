// The guild windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-45..48): the guild window and its five pages are plain classes (the applet registry builds
// them synchronously), the dialogs are lazy classes (`lazy.ts`). 21 layouts become `built`.
import type { WndCtor } from '../../catalog';
import { GuildAppPage } from './GuildAppPage';
import { GuildInfoPage } from './GuildInfoPage';
import { GuildMemberPage } from './GuildMemberPage';
import { GuildPowerPage } from './GuildPowerPage';
import { GuildWarPage } from './GuildWarPage';
import { GUILD_LAZY } from './lazy';
import { WndGuild } from './WndGuild';

export const GUILD_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_GUILD: () => Promise.resolve(WndGuild),
  APP_GUILD_TABINFO: () => Promise.resolve(GuildInfoPage),
  APP_GUILD_TABMEMBER_EX: () => Promise.resolve(GuildMemberPage),
  APP_GUILD_TABAPPELLATION: () => Promise.resolve(GuildAppPage),
  APP_GUILD_TABGUILDWAR: () => Promise.resolve(GuildWarPage),
  APP_GUILD_TAPPOWER: () => Promise.resolve(GuildPowerPage),
  APP_GUILDNAME: () => GUILD_LAZY.WndGuildName.load(),
  APP_GUILD_NICKNAME: () => GUILD_LAZY.WndGuildNickName.load(),
  APP_GUILD_INVATE: () => GUILD_LAZY.WndGuildInvite.load(),
  APP_GUILD_DISMISS: () => GUILD_LAZY.WndGuildDismiss.load(),
  APP_GUILD_NOTICE: () => GUILD_LAZY.WndGuildNotice.load(),
  APP_GUILD_SETLOGO: () => GUILD_LAZY.WndGuildSetLogo.load(),
  APP_GUILD_PAYCONFIRM: () => GUILD_LAZY.WndGuildPayConfirm.load(),
  APP_GUILD_MERIT1: () => GUILD_LAZY.WndGuildMerit.load(),
  APP_GUILD_BANK: () => GUILD_LAZY.WndGuildBank.load(),
  APP_GUILD_BANK_LOG: () => GUILD_LAZY.WndGuildBankLog.load(),
  APP_GUILD_WAR: () => GUILD_LAZY.WndGuildWarDecl.load(),
  APP_GUILD_WARGIVEUP: () => GUILD_LAZY.WndGuildWarGiveUp.load(),
  APP_GUILD_WARPEACE: () => GUILD_LAZY.WndGuildWarPeace.load(),
  APP_GUILD_WARPEACECONFIRM: () => GUILD_LAZY.WndGuildWarPeaceConfirm.load(),
  APP_GUILD_WARREQUEST: () => GUILD_LAZY.WndGuildWarRequest.load(),
};
