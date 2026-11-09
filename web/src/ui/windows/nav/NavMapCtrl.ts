// `NavMapCtrl` (the client area of `CWndNavigator`, `OnEraseBkgnd`; WP-UI-36): a display-only control over the whole client of the
// navigator window that owns the `<canvas>` of the minimap. The window decides WHEN to draw (every UI frame while it is visible); WHAT is
// drawn is the `paint` callback of the port (45's `features/navigator`). The canvas follows the control's size and the device pixel ratio;
// a canvas without a 2-D context (jsdom) is simply not drawn.

import { WTYPE_CUSTOM } from '../../layout/styles';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { viewOf } from '../../controls/bind';
import { Control, type ControlViewState } from '../../wnd/Control';
import { NavMapView } from './NavMapView';
import type { NavSize } from './ports';

export interface NavMapState extends ControlViewState {
  /** The drawing size in UI pixels (the control's rect). */
  w: number;
  h: number;
}

export type NavPaint = (ctx: CanvasRenderingContext2D, size: NavSize, nowMs: number) => void;

export class NavMapCtrl extends Control<NavMapState> {
  /** Draws one frame. */
  paint: NavPaint | null = null;
  #canvas: HTMLCanvasElement | null = null;
  /** How many frames were drawn (tests and the probe). */
  frames = 0;

  constructor() {
    super(WTYPE_CUSTOM, null, { w: 0, h: 0 });
    this.view = viewOf<NavMapCtrl>(NavMapView);
  }

  /** The element the view mounted (`null` while it is not mounted). */
  attachCanvas(canvas: HTMLCanvasElement | null): void {
    this.#canvas = canvas;
  }

  get canvas(): HTMLCanvasElement | null {
    return this.#canvas;
  }

  protected override onRectChanged(): void {
    super.onRectChanged();
    const r = this.rectWindowValue;
    this.state.patch({ w: rectWidth(r), h: rectHeight(r) });
  }

  /** Paints the current frame into the canvas; returns whether anything was drawn. */
  redraw(nowMs: number): boolean {
    const canvas = this.#canvas;
    const paint = this.paint;
    if (!canvas || !paint) return false;
    const s = this.state.get();
    if (s.w <= 0 || s.h <= 0) return false;
    const ctx = canvas.getContext('2d');
    if (!ctx) return false;
    const dpr = canvas.width / Math.max(1, s.w);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, s.w, s.h);
    paint(ctx, { w: s.w, h: s.h }, nowMs);
    this.frames += 1;
    return true;
  }

  /** A display: the window handles the pointer itself. */
  override isPickupSpace(): boolean {
    return false;
  }
}
