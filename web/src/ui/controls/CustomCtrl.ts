// `CWndCustom` (WndControl.cpp:244-250): the placeholder control of `WTYPE_CUSTOM`. A layout placeholder creates
// NOTHING (`getWndCtrl` reads its rect); a controller that instantiates one explicitly (`createControl(WTYPE_CUSTOM,
// ...)`) gets the grey `#FF808080` fill the original paints (KEEP, Plan/42 §8.11).

import type { ControlLayout } from '../layout/registry';
import { WTYPE_CUSTOM } from '../layout/styles';
import { Control } from '../wnd/Control';
import { CustomCtrlView } from './CustomCtrlView';

export class CustomCtrl extends Control {
  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_CUSTOM, layout);
    this.view = CustomCtrlView;
  }
}
