// The party dialogs in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The three applets (the party window,
// the simple display, the finder) and the pages of the party window are plain classes: the applet registry builds them synchronously.
import { LazyWnd } from '../../wnd/lazy';
import type * as ChangeName from './WndPartyChangeName';
import type * as ChangeTroup from './WndPartyChangeTroup';
import type * as Confirm from './WndPartyConfirm';
import type * as LeaveConfirm from './WndPartyLeaveConfirm';
import type * as SummonParty from './WndSummonParty';

export const PARTY_LAZY = {
  WndPartyConfirm: new LazyWnd<typeof Confirm.WndPartyConfirm>(
    'WndPartyConfirm',
    async () => (await import('./WndPartyConfirm')).WndPartyConfirm,
  ),
  WndPartyLeaveConfirm: new LazyWnd<typeof LeaveConfirm.WndPartyLeaveConfirm>(
    'WndPartyLeaveConfirm',
    async () => (await import('./WndPartyLeaveConfirm')).WndPartyLeaveConfirm,
  ),
  WndPartyChangeName: new LazyWnd<typeof ChangeName.WndPartyChangeName>(
    'WndPartyChangeName',
    async () => (await import('./WndPartyChangeName')).WndPartyChangeName,
  ),
  WndPartyChangeTroup: new LazyWnd<typeof ChangeTroup.WndPartyChangeTroup>(
    'WndPartyChangeTroup',
    async () => (await import('./WndPartyChangeTroup')).WndPartyChangeTroup,
  ),
  WndSummonParty: new LazyWnd<typeof SummonParty.WndSummonParty>(
    'WndSummonParty',
    async () => (await import('./WndSummonParty')).WndSummonParty,
  ),
  WndSummonPartyUse: new LazyWnd<typeof SummonParty.WndSummonPartyUse>(
    'WndSummonPartyUse',
    async () => (await import('./WndSummonParty')).WndSummonPartyUse,
  ),
  WndSummonPartyMsg: new LazyWnd<typeof SummonParty.WndSummonPartyMsg>(
    'WndSummonPartyMsg',
    async () => (await import('./WndSummonParty')).WndSummonPartyMsg,
  ),
} as const;
