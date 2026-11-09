// `ChannelSlider` (WP-UI-35 partial): the red, green and blue sliders of the hair shop (`WIDC_CUSTOM_R/G/B`, 120 x 16, value 0..255).
// A horizontal track with a thumb; a press or a drag on the track sets the value from the pointer (`value = round(x / (width - thumb) *
// 255)`), the owner gets `onChange(value)` for every change by the user. `setValue` (the window's sync with the edit box) never calls
// `onChange`.

import { WTYPE_CUSTOM } from '../../layout/styles';
import type { FrameRects, Point, Rect } from '../../layout/geometry';
import { viewOf } from '../../controls/bind';
import { Control, type ControlViewState } from '../../wnd/Control';
import { ChannelSliderView } from './ChannelSliderView';

export const SLIDER_MAX = 255;
export const SLIDER_THUMB = 8;

export interface ChannelSliderState extends ControlViewState {
  value: number;
  /** The colour of the filled part (`rgb(r, 0, 0)` for the red channel...). */
  color: string;
  dragging: boolean;
}

/** The value of a pointer x on a track of `width` px with a thumb of `thumb` px: the thumb centre follows the pointer. */
export function sliderValue(x: number, width: number, thumb = SLIDER_THUMB): number {
  const span = Math.max(1, width - thumb);
  const t = Math.min(1, Math.max(0, (x - thumb / 2) / span));
  return Math.round(t * SLIDER_MAX);
}

/** The left edge of the thumb for a value. */
export function thumbLeft(value: number, width: number, thumb = SLIDER_THUMB): number {
  return Math.round(
    (Math.min(SLIDER_MAX, Math.max(0, value)) / SLIDER_MAX) * Math.max(0, width - thumb),
  );
}

export class ChannelSlider extends Control<ChannelSliderState> {
  onChange: ((value: number) => void) | null = null;

  constructor(color: string) {
    super(WTYPE_CUSTOM, null, { value: 0, color, dragging: false });
    this.view = viewOf<ChannelSlider>(ChannelSliderView);
  }

  /** The whole window is the client (a custom control has no frame): a 16 px track must not lose its rows to the default inset. */
  protected override computeRects(window: Rect): FrameRects {
    return { window: { ...window }, client: { ...window }, layout: { ...window } };
  }

  get value(): number {
    return this.state.get().value;
  }

  setValue(value: number): void {
    this.state.patch({ value: Math.min(SLIDER_MAX, Math.max(0, Math.round(value))) });
  }

  #width(): number {
    const w = this.rectWindowValue;
    return w.r - w.l;
  }

  #apply(pt: Point): void {
    const v = sliderValue(pt.x, this.#width());
    if (v === this.state.get().value) return;
    this.state.patch({ value: v });
    this.onChange?.(v);
  }

  override onLButtonDown(pt: Point): void {
    if (!this.state.get().enabled) return;
    this.state.patch({ dragging: true });
    this.setCapture();
    this.#apply(pt);
  }

  override onMouseMove(pt: Point): void {
    if (this.state.get().dragging) this.#apply(pt);
  }

  override onLButtonUp(): void {
    this.#end();
  }

  override onPointerRelease(): void {
    this.#end();
  }

  #end(): void {
    if (!this.state.get().dragging) return;
    this.state.patch({ dragging: false });
    this.releaseCapture();
  }
}
