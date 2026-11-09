// `confirm(tid, onYes)` (Plan/45 §2.4): the generic yes/no box (`APP_COMMITEM_DIALOG`) of the feature windows, over 42's
// message-box service. The text comes from the texts bundle (`tid`) or is given literally.

import type { MessageBoxApi } from '../../ui/msgbox/MessageBoxService';
import type { TextApi } from '../../data/clientData';

export function confirmTid(
  msgbox: Pick<MessageBoxApi, 'confirm'>,
  texts: TextApi,
  tid: number,
  onYes: () => void,
  onNo?: () => void,
): void {
  msgbox.confirm(texts.text(tid), onYes, onNo);
}
