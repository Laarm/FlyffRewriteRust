// `PaperDoll` (the equipment part of `CWndInventory::OnDraw`, WndField.cpp:1249-1420; WP-UI-21): a display-only control over
// the whole client of the inventory window that paints the worn items at the rects of the layout's `WIDC_CUSTOM1..19`
// placeholders - icon at the rect's top-left plus (6, 6) at 32 px (accessory slots 0.8 x, fashion slots 0.9 x at the
// top-left), ghosted when a two-handed weapon blocks the left hand or the fashion piece is hidden, red when expired, with the
// stack count of stackable items - and the trash can (`WIDC_CUSTOM21`, 27 px, 1.2 x under the pointer). The window computes the
// layers (`buildPaperLayers`) and does all pointer handling itself: clicks, drags, drops and tooltips go through the window's
// own hit tests against the same rects, like the original, so this control never takes the pointer.

import { WTYPE_CUSTOM } from '../../layout/styles';
import { viewOf } from '../../controls/bind';
import { Control, type ControlViewState } from '../../wnd/Control';
import { PaperDollView } from './PaperDollView';

/** One icon of the paper doll. Coordinates are the control's (= the window client's). */
export interface PaperLayer {
  /** Stable identity (`part:<n>` or `trash`). */
  readonly key: string;
  readonly x: number;
  readonly y: number;
  /** The edge of the square icon in px (its width when `height` is given). */
  readonly size: number;
  readonly height?: number;
  /** A path of `Item/...` (`item/itm_x.dds`) or a theme image; `null` draws nothing. */
  readonly icon: string | null;
  /** 0..1 (`dwAlpha / 255`). */
  readonly alpha: number;
  /** Multiplied by (255, 100, 100): an expired item. */
  readonly tint: boolean;
  /** The stack count and the right/top edge it is aligned to. */
  readonly count?: { readonly text: string; readonly right: number; readonly top: number };
}

export interface PaperDollState extends ControlViewState {
  layers: readonly PaperLayer[];
  /** `features.new_font_color`: the colour and font of the counts. */
  newFontColor: boolean;
}

function sameLayer(a: PaperLayer, b: PaperLayer): boolean {
  return (
    a.key === b.key &&
    a.x === b.x &&
    a.y === b.y &&
    a.size === b.size &&
    a.height === b.height &&
    a.icon === b.icon &&
    a.alpha === b.alpha &&
    a.tint === b.tint &&
    a.count?.text === b.count?.text &&
    a.count?.right === b.count?.right &&
    a.count?.top === b.count?.top
  );
}

export class PaperDoll extends Control<PaperDollState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { layers: [], newFontColor: true });
    this.view = viewOf<PaperDoll>(PaperDollView);
  }

  /** Replaces the layers; unchanged lists emit nothing. */
  setLayers(layers: readonly PaperLayer[], newFontColor: boolean): boolean {
    const cur = this.state.get();
    if (
      cur.newFontColor === newFontColor &&
      cur.layers.length === layers.length &&
      cur.layers.every((l, i) => {
        const o = layers[i];
        return o !== undefined && sameLayer(l, o);
      })
    ) {
      return false;
    }
    return this.state.patch({ layers, newFontColor });
  }

  get layers(): readonly PaperLayer[] {
    return this.state.get().layers;
  }

  /** A display: the window's own hit tests handle the pointer. */
  override isPickupSpace(): boolean {
    return false;
  }
}
