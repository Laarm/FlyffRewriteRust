// The server's privacy settings on the client (Plan/45 §0.4, §8.6, PL-095, WP-CFEAT-39): from the first `ChatSettingsNtf` on the browser filters nothing itself. This module has the
// view of the ignore-list window (`CWndChattingBlockingList::UpdateInformation`) and the rule that tells whether the server owns the lists yet.

import {
  TID_GAME_CHATTING_BLOCKING_LIST_TITLE,
  TID_GAME_CHATTING_BLOCKING_USERS_TOTAL_NUMBER,
} from '../../gen/defs/text';
import type { BlockListView } from '../../ui/windows/messenger/ports';
import { formatTemplate } from '../common/text';
import type { ChatSettings, MessengerState } from './store';

/** The server owns the privacy flags and the ignore list once its settings arrived. */
export const serverOwnsSettings = (s: MessengerState): boolean => s.settings !== null;

/** The view of the ignore-list window: the title with the own name, the count, the names in stored order. */
export function blockListView(
  settings: ChatSettings | null,
  ownName: string,
  text: (tid: number) => string,
): BlockListView {
  const names = settings?.ignored ?? [];
  return {
    title: formatTemplate(text(TID_GAME_CHATTING_BLOCKING_LIST_TITLE), [ownName]),
    total: formatTemplate(text(TID_GAME_CHATTING_BLOCKING_USERS_TOTAL_NUMBER), [names.length]),
    names,
  };
}
