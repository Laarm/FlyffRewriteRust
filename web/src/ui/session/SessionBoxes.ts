// The Logout and Quit boxes (`CWndLogOut`, `CWndQuit`, WndField.cpp:12407-12600; Plan/42 §14.2): `MessageBoxWnd`s with the
// OK / Cancel buttons, the title `TID_DIAG_0068` and the text `TID_DIAG_0069` (Logout) or `TID_DIAG_0070` (Quit), placed
// at `((layout.r - w) / 2, 70)`. Cancel closes the box; OK closes it and hands the decision to `SessionActions` (the
// original opened the countdown window or posted `WM_LOGOUT` / `WM_CLOSE` itself).

import { TID_DIAG_0068, TID_DIAG_0069, TID_DIAG_0070 } from '../../gen/defs/text';
import { MessageBoxWnd } from '../msgbox/MessageBoxWnd';
import { MB, MBID } from '../msgbox/types';
import type { SessionActions } from './ports';

export type SessionBoxKind = 'logout' | 'quit';

/** The English texts used while the texts bundle lacks the TIDs. */
const FALLBACK: Readonly<Record<SessionBoxKind, string>> = {
  logout: 'Do you want to disconnect?',
  quit: 'Do you want to exit the program?',
};

export class SessionBox extends MessageBoxWnd {
  readonly kind: SessionBoxKind;

  constructor(
    kind: SessionBoxKind,
    getText: (tid: number) => string,
    actions: () => SessionActions,
  ) {
    const body = getText(kind === 'logout' ? TID_DIAG_0069 : TID_DIAG_0070);
    super(body !== '' ? body : FALLBACK[kind], MB.OKCANCEL, true);
    const title = getText(TID_DIAG_0068);
    if (title !== '') this.setBoxTitle(title);
    this.kind = kind;
    this.onResult = (id) => {
      if (id !== MBID.OK) return;
      const a = actions();
      if (kind === 'logout') a.logout();
      else a.quit();
    };
  }
}
