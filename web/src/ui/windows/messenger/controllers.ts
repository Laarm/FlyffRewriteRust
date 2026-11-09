// The messenger windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-43, WP-UI-44): the applet and its campus page are plain classes (the applet
// registry builds them synchronously), the dialogs are lazy classes (`lazy.ts`).
import type { WndCtor } from '../../catalog';
import { MESSENGER_LAZY } from './lazy';
import { WndCampusTab } from './WndCampusTab';
import { WndMessenger } from './WndMessenger';

export const MESSENGER_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_MESSENGER_EX: () => Promise.resolve(WndMessenger),
  APP_MESSENGER_TAB_CAMPUS: () => Promise.resolve(WndCampusTab),
  APP_MESSAGE: () => MESSENGER_LAZY.WndMessage.load(),
  APP_INSTANTMSG: () => MESSENGER_LAZY.WndInstantMsg.load(),
  APP_MESSAGE_NOTE: () => MESSENGER_LAZY.WndMessageNote.load(),
  APP_MESSENGER_NOTE: () => MESSENGER_LAZY.WndMessengerNote.load(),
  APP_ADDFRIEND: () => MESSENGER_LAZY.WndAddFriend.load(),
  APP_FRIEND_CONFIRM: () => MESSENGER_LAZY.WndFriendConfirm.load(),
  APP_CHATTING_BLOCKING_LIST: () => MESSENGER_LAZY.WndBlockingList.load(),
  APP_SUMMON_FRIEND: () => MESSENGER_LAZY.WndSummonFriend.load(),
  APP_MESSAGEBOX_SUMMONFRINED: () => MESSENGER_LAZY.WndSummonFriendMsg.load(),
};
