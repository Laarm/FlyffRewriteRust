// `Static` (`CWndStatic`, WndControl.cpp:30-250; Plan/42 §8.4, ref/22 §8): a caption at (2, 2) (group box (4, 4)) in the
// default colour `#2E70A9` (white for group boxes), drawn twice at x and x + 1 (faux bold). `WSS_MONEY` formats the
// text with thousands separators, `WSS_ALIGN*` aligns it in the rect, `WSS_PICTURE` draws a 1 px black frame, a group
// box gets the `WndEditTile200` 9-tile skin from the frame's bake. A static is not a hit target of its own for group
// boxes and pictures (clicks go to the parent frame; `Control.isPickupSpace`).

import type { ControlLayout } from '../layout/registry';
import {
  WSS_ALIGNHCENTER,
  WSS_ALIGNHRIGHT,
  WSS_ALIGNVBOTTOM,
  WSS_ALIGNVCENTER,
  WSS_GROUPBOX,
  WTYPE_STATIC,
} from '../layout/styles';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { StaticView } from './StaticView';

/** `m_dwColor` of a plain static (`Create` sets it for non-group statics). */
export const STATIC_COLOR = 0xff2e70a9;
/** `m_dwColor` of a group box (the constructor's default). */
export const GROUPBOX_COLOR = 0xffffffff;

/** The four `WSS_ALIGN*` bits. */
export const STATIC_ALIGN_MASK =
  WSS_ALIGNHRIGHT | WSS_ALIGNHCENTER | WSS_ALIGNVBOTTOM | WSS_ALIGNVCENTER;

export interface StaticState extends ControlViewState {
  /** `0xAARRGGBB`; `null` = the default of the kind (blue, white for group boxes). */
  color: number | null;
}

/**
 * Where the caption starts inside a static of `w x h` px (`CWndStatic::OnDraw`): `(2, 2)` for plain statics, `(4, 4)`
 * for group boxes (raised by `cell - 14` for fonts taller than 14 px); right / centre / bottom / centre-vertical
 * alignment from the measured text size (`textW`, `textH`), never left of / above 2 px.
 */
export function staticTextOrigin(
  style: number,
  w: number,
  h: number,
  textW: number,
  textH: number,
  cell: number,
): { x: number; y: number } {
  const group = (style & WSS_GROUPBOX) !== 0;
  let x = group ? 4 : 2;
  let y = group ? 4 - Math.max(0, cell - 14) : 2;
  if ((style & WSS_ALIGNHRIGHT) !== 0) x = Math.max(2, w - textW - 4);
  if ((style & WSS_ALIGNHCENTER) !== 0) x = Math.max(2, Math.trunc((w - textW) / 2));
  if ((style & WSS_ALIGNVBOTTOM) !== 0) y = Math.max(2, h - textH);
  if ((style & WSS_ALIGNVCENTER) !== 0) y = Math.max(2, Math.trunc((h - textH + 4) / 2));
  return { x, y };
}

export class Static extends Control<StaticState> {
  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_STATIC, layout, { color: null });
    this.view = viewOf<Static>(StaticView);
  }

  /** `m_dwColor` (resolved: the default of the kind when none was set). */
  getTextColor(): number {
    const c = this.state.get().color;
    if (c !== null) return c;
    return (this.style & WSS_GROUPBOX) !== 0 ? GROUPBOX_COLOR : STATIC_COLOR;
  }

  /** `SetTextColor` (`0xAARRGGBB`). */
  setTextColor(color: number): void {
    this.state.patch({ color: color >>> 0 });
  }

  /**
   * Replaces the `WSS_ALIGN*` bits (`SetAlign`): `WSS_ALIGNHRIGHT`, `WSS_ALIGNHCENTER`, `WSS_ALIGNVBOTTOM`,
   * `WSS_ALIGNVCENTER` or 0 for the default top-left.
   */
  setAlign(bits: number): void {
    this.style = (this.style & ~STATIC_ALIGN_MASK) | (bits & STATIC_ALIGN_MASK);
    this.state.patch({ style: this.style });
  }
}
