// The couple dialog in a chunk of its own (`ui/wnd/lazy.ts`): the only place that imports this shell dynamically. The applet (`WndCoupleManager`) and the pages (the two couple tabs and the
// honor tab of the character window) are plain classes: the applet registry and the character window build them synchronously.
import { LazyWnd } from '../../wnd/lazy';
import type * as CoupleMessage from './WndCoupleMessage';

export const COUPLE_LAZY = {
  WndCoupleMessage: new LazyWnd<typeof CoupleMessage.WndCoupleMessage>(
    'WndCoupleMessage',
    async () => (await import('./WndCoupleMessage')).WndCoupleMessage,
  ),
} as const;
