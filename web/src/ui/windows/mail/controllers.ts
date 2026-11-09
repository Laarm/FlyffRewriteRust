// The mail windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-27): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { MAIL_LAZY } from './lazy';

export const MAIL_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_POST: () => MAIL_LAZY.WndPost.load(),
  APP_POST_SEND: () => MAIL_LAZY.WndPostSend.load(),
  APP_POST_RECEIVE: () => MAIL_LAZY.WndPostReceive.load(),
  APP_POST_READ: () => MAIL_LAZY.WndPostRead.load(),
  APP_POST_DELETE_CONFIRM: () => MAIL_LAZY.WndPostDeleteConfirm.load(),
  APP_REQUESTING_POST: () => MAIL_LAZY.WndMailRequesting.load(),
};
