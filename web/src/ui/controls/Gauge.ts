// `Gauge` (`CTheme::RenderGauge`, ITheme.cpp:1684-1906; Plan/42 §8.10, ref/22 §6.8): a bar from a texture of THREE equal
// horizontal slices (left cap, middle, right cap). The width is quantised to whole slices (`n = floor(w / sliceW)`),
// the middle slice is stretched over `n - 2` slices, and nothing is drawn below two slices. A gauge has an empty
// background bar and a fill bar whose width follows `value / max` (HP, MP, FP, EXP of the windows: the textures are
// the window's). Used through `frame.addControl(new Gauge({...}), id, rect)`.

import { WTYPE_CUSTOM } from '../layout/styles';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { GaugeView } from './GaugeView';

export interface GaugeState extends ControlViewState {
  value: number;
  max: number;
  /** The theme file of the empty bar and of the fill (3-slice strips); `null` draws nothing for that layer. */
  background: string | null;
  fill: string | null;
  /** The vertex colour of the fill (`0xAARRGGBB`): its alpha is the opacity, its RGB is multiplied into the texture. */
  color: number;
}

/** How many whole slices fit (`nWidth = rect.Width() / nTexWidth`); under two slices nothing is drawn. */
export function gaugeSlices(width: number, sliceWidth: number): number {
  if (sliceWidth <= 0) return 0;
  const n = Math.floor(width / sliceWidth);
  return n < 2 ? 0 : n;
}

/** The pixel width of a fill: `width * value / max` in integer arithmetic, clamped to `[0, width]`. */
export function gaugeFillWidth(width: number, value: number, max: number): number {
  if (max <= 0 || width <= 0) return 0;
  const v = Math.max(0, Math.min(value, max));
  return Math.trunc((width * v) / max);
}

export interface GaugeOptions {
  background?: string;
  fill?: string;
  color?: number;
}

export class Gauge extends Control<GaugeState> {
  constructor(options: GaugeOptions = {}) {
    super(WTYPE_CUSTOM, null, {
      value: 0,
      max: 0,
      background: options.background ?? null,
      fill: options.fill ?? null,
      color: options.color ?? 0xffffffff,
    });
    this.view = viewOf<Gauge>(GaugeView);
  }

  /** `Gauge.set(value, max)`: diffed, a repeat of the same numbers renders nothing. */
  set(value: number, max: number): void {
    this.state.patch({ value, max });
  }
  setColor(color: number): void {
    this.state.patch({ color: color >>> 0 });
  }
  /** A gauge is a display: clicks pass to the frame. */
  override isPickupSpace(): boolean {
    return false;
  }
}
