// `FillBox`: a code-created control that fills its rect with a translucent colour (the selection highlight of the
// character select: `RenderFillRect(rect, ARGB(20, 80, 250, 80))`, WndTitle.cpp:1675). It paints below the custom canvases
// of the frame, like the original that fills before it renders the model.

import { useSyncExternalStore } from 'react';
import { viewOf } from '../../controls/bind';
import { cssColor } from '../../text/markup';
import { Control, type ControlViewState } from '../../wnd/Control';
import { WTYPE_CUSTOM } from '../../layout/styles';

export interface FillBoxState extends ControlViewState {
  /** `0xAARRGGBB`; alpha 0 draws nothing. */
  color: number;
}

function FillBoxView({ control }: { control: FillBox }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  if (!s.visible || s.color >>> 24 === 0) return null;
  return (
    <div
      className="fillbox"
      data-testid="fillbox"
      style={{
        position: 'absolute',
        inset: 0,
        background: cssColor(s.color),
        pointerEvents: 'none',
      }}
    />
  );
}

export class FillBox extends Control<FillBoxState> {
  constructor(color = 0) {
    super(WTYPE_CUSTOM, null, { color });
    this.view = viewOf<FillBox>(FillBoxView);
  }

  setColor(color: number): void {
    this.state.patch({ color: color >>> 0 });
  }

  /** A fill is never a hit target: the window under it takes the pointer. */
  override isPickupSpace(): boolean {
    return false;
  }
}
