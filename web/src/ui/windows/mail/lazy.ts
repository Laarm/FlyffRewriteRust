// The mail windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shells dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as Requesting from './WndMailRequesting';
import type * as Post from './WndPost';
import type * as DeleteConfirm from './WndPostDeleteConfirm';
import type * as Read from './WndPostRead';
import type * as Receive from './WndPostReceive';
import type * as Send from './WndPostSend';

export const MAIL_LAZY = {
  WndPost: new LazyWnd<typeof Post.WndPost>(
    'WndPost',
    async () => (await import('./WndPost')).WndPost,
  ),
  WndPostSend: new LazyWnd<typeof Send.WndPostSend>(
    'WndPostSend',
    async () => (await import('./WndPostSend')).WndPostSend,
  ),
  WndPostReceive: new LazyWnd<typeof Receive.WndPostReceive>(
    'WndPostReceive',
    async () => (await import('./WndPostReceive')).WndPostReceive,
  ),
  WndPostRead: new LazyWnd<typeof Read.WndPostRead>(
    'WndPostRead',
    async () => (await import('./WndPostRead')).WndPostRead,
  ),
  WndPostDeleteConfirm: new LazyWnd<typeof DeleteConfirm.WndPostDeleteConfirm>(
    'WndPostDeleteConfirm',
    async () => (await import('./WndPostDeleteConfirm')).WndPostDeleteConfirm,
  ),
  WndMailRequesting: new LazyWnd<typeof Requesting.WndMailRequesting>(
    'WndMailRequesting',
    async () => (await import('./WndMailRequesting')).WndMailRequesting,
  ),
} as const;
