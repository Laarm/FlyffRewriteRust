// `ConnectingBox` (`CWndConnectingBox`, `WndTitle.cpp:57-75`): the "Connecting. Please wait." box (`TID_DIAG_0064`, title
// `TID_DIAG_0068`): a message box WITHOUT buttons whose `OnChildNotify` swallows everything. It is closed by the code that
// opened it (`ui.msgbox.closeAll()`, legacy `CloseMessageBox`).

import { MessageBoxWnd } from '../../msgbox/MessageBoxWnd';
import { MB } from '../../msgbox/types';

export class ConnectingBox extends MessageBoxWnd {
  /** @param text `TID_DIAG_0064`; @param title `TID_DIAG_0068` (empty keeps the layout title). */
  constructor(text: string, title = '') {
    super(text, MB.OK);
    if (title !== '') this.setBoxTitle(title);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // no buttons: the box only informs; Enter and clicks do nothing
    for (const b of this.buttons.splice(0)) this.removeControl(b);
  }
}
