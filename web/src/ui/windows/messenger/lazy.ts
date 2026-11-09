// The messenger dialogs in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The messenger itself (an applet) and its pages
// are plain classes: the applet registry builds them synchronously.
import { LazyWnd } from '../../wnd/lazy';
import type * as AddFriend from './WndAddFriend';
import type * as BlockingList from './WndBlockingList';
import type * as FriendConfirm from './WndFriendConfirm';
import type * as InstantMsg from './WndInstantMsg';
import type * as Message from './WndMessage';
import type * as MessageNote from './WndMessageNote';
import type * as MessengerNote from './WndMessengerNote';
import type * as SummonFriend from './WndSummonFriend';

export const MESSENGER_LAZY = {
  WndMessage: new LazyWnd<typeof Message.WndMessage>(
    'WndMessage',
    async () => (await import('./WndMessage')).WndMessage,
  ),
  WndInstantMsg: new LazyWnd<typeof InstantMsg.WndInstantMsg>(
    'WndInstantMsg',
    async () => (await import('./WndInstantMsg')).WndInstantMsg,
  ),
  WndMessageNote: new LazyWnd<typeof MessageNote.WndMessageNote>(
    'WndMessageNote',
    async () => (await import('./WndMessageNote')).WndMessageNote,
  ),
  WndMessengerNote: new LazyWnd<typeof MessengerNote.WndMessengerNote>(
    'WndMessengerNote',
    async () => (await import('./WndMessengerNote')).WndMessengerNote,
  ),
  WndAddFriend: new LazyWnd<typeof AddFriend.WndAddFriend>(
    'WndAddFriend',
    async () => (await import('./WndAddFriend')).WndAddFriend,
  ),
  WndFriendConfirm: new LazyWnd<typeof FriendConfirm.WndFriendConfirm>(
    'WndFriendConfirm',
    async () => (await import('./WndFriendConfirm')).WndFriendConfirm,
  ),
  WndBlockingList: new LazyWnd<typeof BlockingList.WndBlockingList>(
    'WndBlockingList',
    async () => (await import('./WndBlockingList')).WndBlockingList,
  ),
  WndSummonFriend: new LazyWnd<typeof SummonFriend.WndSummonFriend>(
    'WndSummonFriend',
    async () => (await import('./WndSummonFriend')).WndSummonFriend,
  ),
  WndSummonFriendMsg: new LazyWnd<typeof SummonFriend.WndSummonFriendMsg>(
    'WndSummonFriendMsg',
    async () => (await import('./WndSummonFriend')).WndSummonFriendMsg,
  ),
} as const;
