// `OverlayLayer`: the DOM side of the overlay (Plan/41 §19.1): a 2-D canvas stacked above the world canvas, sized in CSS
// pixels with the device pixel ratio folded into its backing store, never taking pointer events. The world render host
// creates one next to the WebGL canvas, hands `layer.renderer` to `composeFx` and calls `layer.renderer.draw(...)` after
// `WorldRenderer.draw` every frame.

import { OverlayRenderer, type OverlayRendererOptions } from './OverlayRenderer';

export interface OverlayLayerOptions extends Omit<OverlayRendererOptions, 'ctx'> {
  /** The canvas of the WebGL renderer; the overlay is inserted right after it and tracks its CSS box. */
  readonly worldCanvas: HTMLCanvasElement;
  /** Cap of the backing-store scale (`RenderOptions.dprCap`). */
  readonly dprCap?: number;
}

export class OverlayLayer {
  readonly canvas: HTMLCanvasElement;
  readonly renderer: OverlayRenderer;
  readonly #ctx: CanvasRenderingContext2D;
  readonly #dprCap: number;
  #cssW = 0;
  #cssH = 0;

  /** `null` when the browser gives no 2-D context. */
  static create(o: OverlayLayerOptions): OverlayLayer | null {
    const doc = o.worldCanvas.ownerDocument;
    const canvas = doc.createElement('canvas');
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;
    return new OverlayLayer(o, canvas, ctx);
  }

  private constructor(
    o: OverlayLayerOptions,
    canvas: HTMLCanvasElement,
    ctx: CanvasRenderingContext2D,
  ) {
    this.canvas = canvas;
    this.#ctx = ctx;
    this.#dprCap = o.dprCap ?? 1.5;
    canvas.dataset['testid'] = 'world-overlay';
    canvas.style.cssText =
      'position:absolute;left:0;top:0;width:100%;height:100%;pointer-events:none;image-rendering:auto';
    o.worldCanvas.after(canvas);
    this.renderer = new OverlayRenderer({ ...o, ctx });
  }

  /** Follows the CSS box of the world canvas: the backing store is `css * min(dpr, cap)`, the transform maps CSS pixels. */
  resize(cssW: number, cssH: number, dpr: number): void {
    if (cssW === this.#cssW && cssH === this.#cssH && this.canvas.width > 0) return;
    this.#cssW = cssW;
    this.#cssH = cssH;
    const scale = Math.max(1, Math.min(dpr, this.#dprCap));
    this.canvas.width = Math.round(cssW * scale);
    this.canvas.height = Math.round(cssH * scale);
    this.#ctx.setTransform(scale, 0, 0, scale, 0, 0);
    this.renderer.resize(cssW, cssH);
  }

  dispose(): void {
    this.renderer.clear();
    this.canvas.remove();
  }
}
