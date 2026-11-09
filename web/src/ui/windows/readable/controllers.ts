// The readable-item windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-37): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { READABLE_LAZY } from './lazy';

export const READABLE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_TEXT_BOOK: () => READABLE_LAZY.WndTextBook.load(),
  APP_TEXT_SCROLL: () => READABLE_LAZY.WndTextScroll.load(),
  APP_TEXT_LETTER: () => READABLE_LAZY.WndTextLetter.load(),
};
