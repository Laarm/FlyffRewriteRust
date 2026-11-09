// `TextLayer` (Plan/42 §4.6 `TextOut` in `OnDraw`; WP-UI-21): a display-only control that paints independently positioned
// text items over a window or a tab page - the status numbers, the whole character information page. The window computes the
// items from its view (`setItems`, diffed: an unchanged list emits nothing, so a 10 Hz poll costs no React commit).
// Positions are the top-left of the text box in the layer's own coordinates, exactly the `TextOut(x, y, ...)` arguments.

import { WTYPE_CUSTOM } from '../layout/styles';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { TextLayerView } from './TextLayerView';

/** The fonts a layer can use: `FONT.text` and the outlined `FONT.title` (`m_pFontWndTitle`). */
export type TextLayerFont = 'text' | 'title';

export interface TextItem {
  readonly x: number;
  readonly y: number;
  readonly text: string;
  /** `0xAARRGGBB`. */
  readonly color: number;
  readonly font?: TextLayerFont;
}

export interface TextLayerState extends ControlViewState {
  items: readonly TextItem[];
}

/** Element-wise equality of two item lists. */
export function sameItems(a: readonly TextItem[], b: readonly TextItem[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const p = a[i];
    const q = b[i];
    if (p?.x !== q?.x || p?.y !== q?.y || p?.text !== q?.text) return false;
    if (p?.color !== q?.color || p?.font !== q?.font) return false;
  }
  return true;
}

export class TextLayer extends Control<TextLayerState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { items: [] });
    this.view = viewOf<TextLayer>(TextLayerView);
  }

  /** Replaces the items; returns `false` (and emits nothing) when they are the same as before. */
  setItems(items: readonly TextItem[]): boolean {
    if (sameItems(this.state.get().items, items)) return false;
    return this.state.patch({ items });
  }

  get items(): readonly TextItem[] {
    return this.state.get().items;
  }

  /** A display: presses and drops pass to the window below. */
  override isPickupSpace(): boolean {
    return false;
  }
}
