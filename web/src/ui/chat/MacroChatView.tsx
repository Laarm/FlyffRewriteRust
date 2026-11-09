// The pixels of the macro icon of the chat window (`CWndMacroChat::OnDraw`): `wndButtMacroChat.bmp` while the edit has text.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { frameBackground } from '../controls/sprites';
import { useSprite } from '../controls/useSprite';
import type { MacroChatCtrl } from './MacroChatCtrl';

export const MACRO_CHAT_IMAGE = 'wndButtMacroChat.bmp';

export function MacroChatView({ control }: { control: MacroChatCtrl }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const sprite = useSprite(control.env, s.shown ? MACRO_CHAT_IMAGE : null);
  if (!s.shown || typeof sprite === 'string') return null;
  return (
    <div
      data-testid="macro-chat"
      style={{
        width: rectWidth(s.rect),
        height: rectHeight(s.rect),
        ...frameBackground(sprite, 1, 0),
      }}
    />
  );
}
