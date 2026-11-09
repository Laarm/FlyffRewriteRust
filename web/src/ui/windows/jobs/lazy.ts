// The job windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shells dynamically. They open with the class-change scroll
// and the fast job change item, never from the taskbar.
import { LazyWnd } from '../../wnd/lazy';
import type * as ChangeClass from './WndChangeClass';
import type * as FastJobChange from './WndFastJobChange';

export const JOBS_LAZY = {
  WndFastJobChange: new LazyWnd<typeof FastJobChange.WndFastJobChange>(
    'WndFastJobChange',
    async () => (await import('./WndFastJobChange')).WndFastJobChange,
  ),
  WndChangeClass1: new LazyWnd<typeof ChangeClass.WndChangeClass1>(
    'WndChangeClass1',
    async () => (await import('./WndChangeClass')).WndChangeClass1,
  ),
  WndChangeClass2: new LazyWnd<typeof ChangeClass.WndChangeClass2>(
    'WndChangeClass2',
    async () => (await import('./WndChangeClass')).WndChangeClass2,
  ),
} as const;
