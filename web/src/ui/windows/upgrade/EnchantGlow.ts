// `EnchantGlow` (`RenderEnchant( p2DRender, pt )`, the upgrade-wait glow of `CWndNewUpgrade::OnDraw`; WP-UI-29): a display-only control that
// paints the 32 x 32 enchant strip animation (`.flyff-enchant`, a CSS animation, no per-frame JavaScript) over the icons of its window. The
// window sets the spots while an upgrade runs and clears them when it ends; an unchanged list emits nothing.

import { viewOf } from '../../controls/bind';
import { WTYPE_CUSTOM } from '../../layout/styles';
import { Control, type ControlViewState } from '../../wnd/Control';
import { EnchantGlowView } from './EnchantGlowView';

export interface GlowSpot {
  readonly x: number;
  readonly y: number;
}

export interface EnchantGlowState extends ControlViewState {
  spots: readonly GlowSpot[];
}

export class EnchantGlow extends Control<EnchantGlowState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { spots: [] });
    this.view = viewOf<EnchantGlow>(EnchantGlowView);
  }

  /** Replaces the spots; returns `false` (and emits nothing) when they are the same as before. */
  setSpots(spots: readonly GlowSpot[]): boolean {
    const cur = this.state.get().spots;
    const key = (l: readonly GlowSpot[]): string =>
      l.map((p) => `${String(p.x)},${String(p.y)}`).join(';');
    if (key(cur) === key(spots)) return false;
    return this.state.patch({ spots });
  }

  get spots(): readonly GlowSpot[] {
    return this.state.get().spots;
  }

  /** A display: presses and drops pass to the window below. */
  override isPickupSpace(): boolean {
    return false;
  }
}
