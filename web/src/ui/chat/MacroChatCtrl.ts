// `CWndMacroChat` (WndCommunication.cpp:77-130): the small icon left of the chat edit. While the edit holds text it shows
// `wndButtMacroChat.bmp`; pressing it starts a `SHORTCUT_CHAT` drag that carries the edit text (at most 127 bytes), which
// the taskbar slots accept (a one-click chat macro, Plan/42 §10.2).

import { WTYPE_CUSTOM } from '../layout/styles';
import { viewOf } from '../controls/bind';
import { SC } from '../dnd/shortcut';
import { Control, type ControlViewState } from '../wnd/Control';
import { MacroChatView } from './MacroChatView';

/** `MAX_SHORTCUT_STRING - 1`: the text a macro keeps. */
export const MACRO_MAX_BYTES = 127;

export interface MacroChatState extends ControlViewState {
  /** The edit is not empty: the icon is drawn and can be dragged. */
  shown: boolean;
}

/** The first `max` UTF-8 bytes of `s`, cut at a character boundary. */
export function cutBytes(s: string, max: number): string {
  const enc = new TextEncoder();
  if (enc.encode(s).length <= max) return s;
  let out = '';
  let n = 0;
  for (const ch of s) {
    const len = enc.encode(ch).length;
    if (n + len > max) break;
    out += ch;
    n += len;
  }
  return out;
}

export class MacroChatCtrl extends Control<MacroChatState> {
  /** The edit text the drag carries (`GetTextFormat` of the edit; the web edit has no markup). */
  readonly text: () => string;

  constructor(text: () => string) {
    super(WTYPE_CUSTOM, null, { shown: false });
    this.text = text;
    this.view = viewOf<MacroChatCtrl>(MacroChatView);
  }

  setShown(shown: boolean): void {
    this.state.patch({ shown });
  }

  override onLButtonDown(): void {
    if (!this.state.get().shown) return;
    const text = cutBytes(this.text(), MACRO_MAX_BYTES);
    this.env.wm.setDraggedShortcut({
      kind: SC.CHAT,
      id: 0,
      type: 0,
      index: 0,
      userId: 0,
      data: 0,
      text,
      from: this,
      icon: 'icon/icon_MacroChat.dds',
    });
  }
}
