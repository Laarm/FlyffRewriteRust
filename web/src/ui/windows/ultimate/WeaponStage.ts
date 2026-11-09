// `WeaponStage` (WP-UI-32; the `WIDC_CUSTOM1` canvas of `CWndSmeltJewel::OnDraw`, `src:_Interface/WndField.cpp:20690-20900`): a display-only control
// that shows the weapon of the jewel setting window. The original draws the 3-D model of the weapon turning on the X axis (2 degrees per frame),
// and after Start the turning accelerates for about six seconds (`m_fAddRot += 4` at each of 26 steps), the camera moves in and a pale
// quad fades over it. There is no item-model preview in the renderer yet (WP-REND-06 draws movers only), so the stage turns the weapon's ICON the
// same way, with the same timeline (`spinTimeline`): CSS animations keyed by the start, never per-frame JavaScript (Plan/42 §2.4 rule 3).

import { WTYPE_CUSTOM } from '../../layout/styles';
import { viewOf } from '../../controls/bind';
import { Control, type ControlViewState } from '../../wnd/Control';
import { spinTimeline, type SpinTimeline } from './spin';
import { WeaponStageView } from './WeaponStageView';

export interface WeaponStageState extends ControlViewState {
  /** The weapon icon (`item/itm_x.dds`), `null` = nothing in the window. */
  icon: string | null;
  /** The accelerating spin is running (`m_nStatus == 1`). */
  spinning: boolean;
  /** The CSS durations of the running spin. */
  spin: SpinTimeline;
}

export class WeaponStage extends Control<WeaponStageState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { icon: null, spinning: false, spin: spinTimeline() });
    this.view = viewOf<WeaponStage>(WeaponStageView);
  }

  setIcon(icon: string | null): boolean {
    return this.state.patch({ icon });
  }

  setSpinning(spinning: boolean): boolean {
    return this.state.patch({ spinning });
  }

  get icon(): string | null {
    return this.state.get().icon;
  }

  get spinning(): boolean {
    return this.state.get().spinning;
  }

  /** A display: presses and drops pass to the window below. */
  override isPickupSpace(): boolean {
    return false;
  }
}
