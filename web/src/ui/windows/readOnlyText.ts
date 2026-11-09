// A read-only wrapped text where a layout has a one-line `WTYPE_EDITCTRL` (the question of `APP_STATE_CONFIRM`, the warning of
// `APP_QUEITMWARNING`): the original casts these edits to `CWndText`, which wraps and keeps newlines; our `EditCtrl` is a
// native single-line input, so the shell swaps it for a `TextCtrl` at the same rect with the same skin.

import type { TextCtrl } from '../controls/TextCtrl';
import { WTYPE_TEXT } from '../layout/styles';
import type { WidcKey } from '../layout/registry';
import type { WndNeuz } from '../wnd/WndNeuz';

/** Replaces the layout control `id` by a `TextCtrl` of the same rect and skin; `null` when the layout has no such control. */
export function replaceWithText(frame: WndNeuz, id: WidcKey): TextCtrl | null {
  const old = frame.getDlgItem(id);
  const rect = frame.getWndCtrl(id)?.rect;
  if (!old || !rect) return null;
  const { skinTexture, skinTile, id: oldId } = old;
  frame.removeControl(old);
  const text = frame.createControl<TextCtrl>(WTYPE_TEXT, oldId, rect, 0);
  text.setSkin(skinTexture, skinTile);
  return text;
}
