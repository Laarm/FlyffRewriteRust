// The guild dialogs in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The guild window (an applet of the registry) and its
// five pages are plain classes: the applet registry builds them synchronously. One class per line (the vitest transform `lazyWndSync` reads this file line by line).
import { LazyWnd } from '../../wnd/lazy';
import type * as Bank from './WndGuildBank';
import type * as BankLog from './WndGuildBankLog';
import type * as Dismiss from './WndGuildDismiss';
import type * as Invite from './WndGuildInvite';
import type * as Merit from './WndGuildMerit';
import type * as Name from './WndGuildName';
import type * as Notice from './WndGuildNotice';
import type * as PayConfirm from './WndGuildPayConfirm';
import type * as SetLogo from './WndGuildSetLogo';
import type * as WarDecl from './WndGuildWarDecl';
import type * as WarGiveUp from './WndGuildWarGiveUp';
import type * as WarPeace from './WndGuildWarPeace';
import type * as WarPeaceConfirm from './WndGuildWarPeaceConfirm';
import type * as WarRequest from './WndGuildWarRequest';

export const GUILD_LAZY = {
  WndGuildInvite: new LazyWnd<typeof Invite.WndGuildInvite>(
    'WndGuildInvite',
    async () => (await import('./WndGuildInvite')).WndGuildInvite,
  ),
  WndGuildName: new LazyWnd<typeof Name.WndGuildName>(
    'WndGuildName',
    async () => (await import('./WndGuildName')).WndGuildName,
  ),
  WndGuildNickName: new LazyWnd<typeof Name.WndGuildNickName>(
    'WndGuildNickName',
    async () => (await import('./WndGuildName')).WndGuildNickName,
  ),
  WndGuildDismiss: new LazyWnd<typeof Dismiss.WndGuildDismiss>(
    'WndGuildDismiss',
    async () => (await import('./WndGuildDismiss')).WndGuildDismiss,
  ),
  WndGuildNotice: new LazyWnd<typeof Notice.WndGuildNotice>(
    'WndGuildNotice',
    async () => (await import('./WndGuildNotice')).WndGuildNotice,
  ),
  WndGuildSetLogo: new LazyWnd<typeof SetLogo.WndGuildSetLogo>(
    'WndGuildSetLogo',
    async () => (await import('./WndGuildSetLogo')).WndGuildSetLogo,
  ),
  WndGuildPayConfirm: new LazyWnd<typeof PayConfirm.WndGuildPayConfirm>(
    'WndGuildPayConfirm',
    async () => (await import('./WndGuildPayConfirm')).WndGuildPayConfirm,
  ),
  WndGuildMerit: new LazyWnd<typeof Merit.WndGuildMerit>(
    'WndGuildMerit',
    async () => (await import('./WndGuildMerit')).WndGuildMerit,
  ),
  WndGuildBank: new LazyWnd<typeof Bank.WndGuildBank>(
    'WndGuildBank',
    async () => (await import('./WndGuildBank')).WndGuildBank,
  ),
  WndGuildBankLog: new LazyWnd<typeof BankLog.WndGuildBankLog>(
    'WndGuildBankLog',
    async () => (await import('./WndGuildBankLog')).WndGuildBankLog,
  ),
  WndGuildWarDecl: new LazyWnd<typeof WarDecl.WndGuildWarDecl>(
    'WndGuildWarDecl',
    async () => (await import('./WndGuildWarDecl')).WndGuildWarDecl,
  ),
  WndGuildWarGiveUp: new LazyWnd<typeof WarGiveUp.WndGuildWarGiveUp>(
    'WndGuildWarGiveUp',
    async () => (await import('./WndGuildWarGiveUp')).WndGuildWarGiveUp,
  ),
  WndGuildWarPeace: new LazyWnd<typeof WarPeace.WndGuildWarPeace>(
    'WndGuildWarPeace',
    async () => (await import('./WndGuildWarPeace')).WndGuildWarPeace,
  ),
  WndGuildWarPeaceConfirm: new LazyWnd<typeof WarPeaceConfirm.WndGuildWarPeaceConfirm>(
    'WndGuildWarPeaceConfirm',
    async () => (await import('./WndGuildWarPeaceConfirm')).WndGuildWarPeaceConfirm,
  ),
  WndGuildWarRequest: new LazyWnd<typeof WarRequest.WndGuildWarRequest>(
    'WndGuildWarRequest',
    async () => (await import('./WndGuildWarRequest')).WndGuildWarRequest,
  ),
} as const;
