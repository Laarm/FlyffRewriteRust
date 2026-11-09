// The readable-item windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shell module dynamically. They are opened with a
// request object (`ui.readable.open`) when an item with a text file is read; no applet registry builds them.
import { LazyWnd } from '../../wnd/lazy';
import type * as Readable from './WndReadable';

export const READABLE_LAZY = {
  WndTextBook: new LazyWnd<typeof Readable.WndTextBook>(
    'WndTextBook',
    async () => (await import('./WndReadable')).WndTextBook,
  ),
  WndTextScroll: new LazyWnd<typeof Readable.WndTextScroll>(
    'WndTextScroll',
    async () => (await import('./WndReadable')).WndTextScroll,
  ),
  WndTextLetter: new LazyWnd<typeof Readable.WndTextLetter>(
    'WndTextLetter',
    async () => (await import('./WndReadable')).WndTextLetter,
  ),
} as const;
