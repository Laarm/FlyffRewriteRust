// `RectLayer` (WP-UI-35; the `RenderFillRect` / `RenderRect` overlays of the appearance windows: the selection of a face card
// (`0x60ffff00` over the card), the glow around the chosen element icon of `CWndChangeAttribute::FillRect`, the frame round the chosen
// face of `CWndChangeFace`): a display-only control that paints independently positioned rectangles - a translucent fill, an outline, a
// soft glow - above the other controls of its window. The window computes the entries from its view and `setEntries` diffs them: an
// unchanged list emits nothing. Entries are painted in list order; the control never takes the pointer.

import { WTYPE_CUSTOM } from '../layout/styles';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { RectLayerView } from './RectLayerView';

export interface RectEntry {
  /** A stable identity (the React key). */
  readonly key: string;
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
  /** A CSS colour for the fill (`rgba(255, 255, 0, 0.376)` for `0x60ffff00`); absent = none. */
  readonly fill?: string;
  /** The outline: a CSS colour and the width in px (the original draws one pixel rectangles, `RenderRect`). */
  readonly outline?: { readonly color: string; readonly width: number };
  /** A soft glow outside the rectangle (`FillRect` of the element window: a gradient of 8 px that fades to transparent). */
  readonly glow?: { readonly color: string; readonly size: number };
}

export interface RectLayerState extends ControlViewState {
  entries: readonly RectEntry[];
}

/** Element-wise equality of two entry lists. */
export function sameRects(a: readonly RectEntry[], b: readonly RectEntry[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const p = a[i];
    const q = b[i];
    if (!p || !q) return false;
    if (p.key !== q.key || p.x !== q.x || p.y !== q.y || p.w !== q.w || p.h !== q.h) return false;
    if (p.fill !== q.fill) return false;
    if (p.outline?.color !== q.outline?.color || p.outline?.width !== q.outline?.width)
      return false;
    if (p.glow?.color !== q.glow?.color || p.glow?.size !== q.glow?.size) return false;
  }
  return true;
}

export class RectLayer extends Control<RectLayerState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { entries: [] });
    this.view = viewOf<RectLayer>(RectLayerView);
  }

  /** Replaces the entries; returns `false` (and emits nothing) when they are the same as before. */
  setEntries(entries: readonly RectEntry[]): boolean {
    if (sameRects(this.state.get().entries, entries)) return false;
    return this.state.patch({ entries });
  }

  get entries(): readonly RectEntry[] {
    return this.state.get().entries;
  }

  /** A display: presses and drops pass to the window below. */
  override isPickupSpace(): boolean {
    return false;
  }
}

/** The CSS colour of an `0xAARRGGBB` value. */
export function argbCss(argb: number): string {
  const a = ((argb >>> 24) & 255) / 255;
  const r = (argb >>> 16) & 255;
  const g = (argb >>> 8) & 255;
  const b = argb & 255;
  return `rgba(${String(r)}, ${String(g)}, ${String(b)}, ${String(Math.round(a * 1000) / 1000)})`;
}
