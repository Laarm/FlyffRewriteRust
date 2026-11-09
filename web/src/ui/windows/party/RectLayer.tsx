// `RectLayer`: a display-only control that fills and outlines independently positioned rectangles (`RenderFillRect` / `RenderRect` of the
// `OnDraw` of the party tab: the status squares and the highlight of the selected row). The window computes the entries from its view and
// `setRects` diffs them, like `TextLayer`.

import { useSyncExternalStore } from 'react';
import { viewOf } from '../../controls/bind';
import { WTYPE_CUSTOM } from '../../layout/styles';
import { cssColor } from '../../text/markup';
import { Control, type ControlViewState } from '../../wnd/Control';

export interface RectEntry {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
  /** `0xAARRGGBB` of the fill; absent = no fill. */
  readonly fill?: number;
  /** `0xAARRGGBB` of the 1 px outline; absent = none. */
  readonly stroke?: number;
}

export interface RectLayerState extends ControlViewState {
  rects: readonly RectEntry[];
}

export function sameRects(a: readonly RectEntry[], b: readonly RectEntry[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const p = a[i];
    const q = b[i];
    if (!p || !q) return false;
    if (p.x !== q.x || p.y !== q.y || p.w !== q.w || p.h !== q.h) return false;
    if (p.fill !== q.fill || p.stroke !== q.stroke) return false;
  }
  return true;
}

export class RectLayer extends Control<RectLayerState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { rects: [] });
    this.view = viewOf<RectLayer>(RectLayerView);
  }

  /** Replaces the rectangles; returns `false` (and emits nothing) when they are the same as before. */
  setRects(rects: readonly RectEntry[]): boolean {
    if (sameRects(this.state.get().rects, rects)) return false;
    return this.state.patch({ rects });
  }

  get rects(): readonly RectEntry[] {
    return this.state.get().rects;
  }

  /** A display: presses and drops pass to the window below. */
  override isPickupSpace(): boolean {
    return false;
  }
}

function RectLayerView({ control }: { control: RectLayer }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  return (
    <div data-testid="rect-layer" style={{ position: 'relative' }}>
      {s.rects.map((r, i) => (
        <div
          // the list is rebuilt from a fixed layout: the index is the identity
          key={i}
          className="rl-item"
          style={{
            position: 'absolute',
            left: r.x,
            top: r.y,
            width: r.w,
            height: r.h,
            boxSizing: 'border-box',
            ...(r.fill === undefined ? {} : { background: cssColor(r.fill) }),
            ...(r.stroke === undefined ? {} : { border: `1px solid ${cssColor(r.stroke)}` }),
          }}
        />
      ))}
    </div>
  );
}
