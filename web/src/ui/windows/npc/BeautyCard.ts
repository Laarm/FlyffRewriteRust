// `BeautyCard` (WP-UI-35): one card of the hair and face shops. The originals put a 3-D preview of the model at each `WTYPE_CUSTOM`
// placeholder (`customCanvas(id)`, doc 41 `PreviewRenderer`) and frame the selected card with a translucent yellow fill
// (`RenderFillRect( rect, 0x60ffff00 )`); a click on a card picks it. With a preview behind it (`setOverlay(true)`) the card draws
// ONLY that fill, above the canvas; without one (no WebGL2, tests) it is a plain framed box with a label (`Style 3`, `Before`) and an
// optional colour swatch of the draft hair colour.

import { WTYPE_CUSTOM } from '../../layout/styles';
import type { FrameRects, Point, Rect } from '../../layout/geometry';
import { viewOf } from '../../controls/bind';
import { Control, type ControlViewState } from '../../wnd/Control';
import { BeautyCardView } from './BeautyCardView';

export interface BeautyCardState extends ControlViewState {
  label: string;
  /** `0xRRGGBB` of the swatch, `null` for none. */
  swatch: number | null;
  selected: boolean;
  hover: boolean;
  /** A click on the card picks it (a card that only displays has no handler). */
  pickable: boolean;
  /** A 3-D preview sits behind the card: only the selection fill is drawn (above the canvas). */
  overlay: boolean;
}

export class BeautyCard extends Control<BeautyCardState> {
  /** The card was clicked (a press and release on it). */
  onPick: (() => void) | null = null;
  #pressed = false;

  constructor() {
    super(WTYPE_CUSTOM, null, {
      label: '',
      swatch: null,
      selected: false,
      hover: false,
      pickable: false,
      overlay: false,
    });
    this.view = viewOf<BeautyCard>(BeautyCardView);
  }

  /** The whole window is the client (a custom control has no frame). */
  protected override computeRects(window: Rect): FrameRects {
    return { window: { ...window }, client: { ...window }, layout: { ...window } };
  }

  /** Diffed setters: an unchanged value emits nothing. */
  set(label: string, swatch: number | null, selected: boolean): void {
    this.state.patch({ label, swatch, selected });
  }

  setPickable(pickable: boolean): void {
    this.state.patch({ pickable });
  }

  /** `true` when a 3-D preview shows the model: the card keeps only the selection fill. */
  setOverlay(overlay: boolean): void {
    this.state.patch({ overlay });
  }

  override onLButtonDown(): void {
    this.#pressed = this.state.get().pickable;
  }

  override onLButtonUp(pt: Point): void {
    const was = this.#pressed;
    this.#pressed = false;
    const w = this.rectWindowValue;
    if (!was || pt.x < 0 || pt.y < 0 || pt.x >= w.r - w.l || pt.y >= w.b - w.t) return;
    this.onPick?.();
  }

  override onPointerRelease(): void {
    this.#pressed = false;
  }

  override onMouseEnter(): void {
    this.state.patch({ hover: true });
  }

  override onMouseLeave(): void {
    this.state.patch({ hover: false });
  }
}
