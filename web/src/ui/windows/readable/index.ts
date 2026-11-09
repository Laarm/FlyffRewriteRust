// `ReadableWindowsService` (`ui.readable`, Plan/42 §19.4 D; WP-UI-37): the book, scroll and letter windows.
//
//   ui.readable.open('book' | 'scroll' | 'letter', request)    APP_TEXT_BOOK / APP_TEXT_SCROLL / APP_TEXT_LETTER (`OpenTextBook` ...); each replaces its own previous window

import { APP_TEXT_BOOK, APP_TEXT_LETTER, APP_TEXT_SCROLL } from '../../../gen/defs/resdata';
import { openLazy } from '../../wnd/lazy';
import type { WindowManager } from '../../wm/WindowManager';
import { READABLE_LAZY } from './lazy';
import type { ReadableRequest } from './ports';
import type { WndTextBook, WndTextLetter, WndTextScroll } from './WndReadable';

export * from './ports';
export { READABLE_PICTURE } from './constants';
export type { WndTextBook, WndTextLetter, WndTextScroll } from './WndReadable';

export type ReadableKind = 'book' | 'scroll' | 'letter';

export interface ReadableWindowDeps {
  readonly wm: WindowManager;
}

const APPS: Readonly<Record<ReadableKind, number>> = {
  book: APP_TEXT_BOOK,
  scroll: APP_TEXT_SCROLL,
  letter: APP_TEXT_LETTER,
};

export class ReadableWindowsService {
  readonly #wm: WindowManager;

  constructor(deps: ReadableWindowDeps) {
    this.#wm = deps.wm;
  }

  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (old) {
      this.#wm.destroy(old);
      this.#wm.flushDestroyed();
    }
  }

  /**
   * Opens the window of `kind` (`SAFE_DELETE( m_pWndTextBook ); new CWndTextBook`). `null` while the chunk of the shell is still loading: the
   * window then opens by itself.
   */
  open(
    kind: ReadableKind,
    request: ReadableRequest,
  ): WndTextBook | WndTextScroll | WndTextLetter | null {
    const replace = (a: number): void => {
      this.#replace(a);
    };
    switch (kind) {
      case 'book':
        return openLazy(
          this.#wm,
          APPS.book,
          READABLE_LAZY.WndTextBook,
          (C) => new C(request),
          replace,
        );
      case 'scroll':
        return openLazy(
          this.#wm,
          APPS.scroll,
          READABLE_LAZY.WndTextScroll,
          (C) => new C(request),
          replace,
        );
      case 'letter':
        return openLazy(
          this.#wm,
          APPS.letter,
          READABLE_LAZY.WndTextLetter,
          (C) => new C(request),
          replace,
        );
    }
  }

  /** The open window of `kind`, if any. */
  get(kind: ReadableKind): WndTextBook | WndTextScroll | WndTextLetter | null {
    const w = this.#wm.get(APPS[kind]);
    if (w === null) return null;
    for (const lazy of [
      READABLE_LAZY.WndTextBook,
      READABLE_LAZY.WndTextScroll,
      READABLE_LAZY.WndTextLetter,
    ]) {
      const C = lazy.ctor;
      if (C && w instanceof C) return w;
    }
    return null;
  }
}
