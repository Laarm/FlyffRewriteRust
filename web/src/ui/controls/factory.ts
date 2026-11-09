// The control factory: which class implements which `WTYPE_*` (Plan/42 §8). Kinds without a class keep the
// placeholder `Control` + `GenericControlView`; the work packages replace the arms (WP-UI-05 Static / Text /
// ScrollBar, WP-UI-06 Button / Edit, WP-UI-07 ListBox / ComboBox / TabCtrl / TreeCtrl / Menu).

import type { ControlLayout } from '../layout/registry';
import type { ControlKind } from '../layout/styles';
import {
  WTYPE_BUTTON,
  WTYPE_COMBOBOX,
  WTYPE_CUSTOM,
  WTYPE_EDITCTRL,
  WTYPE_LISTBOX,
  WTYPE_SCROLLBAR,
  WTYPE_STATIC,
  WTYPE_TABCTRL,
  WTYPE_TEXT,
  WTYPE_TREECTRL,
} from '../layout/styles';
import { Control } from '../wnd/Control';
import { Button } from './Button';
import { ComboBox } from './ComboBox';
import { CustomCtrl } from './CustomCtrl';
import { EditCtrl } from './EditCtrl';
import { GenericControlView } from './GenericControlView';
import { ListBox } from './ListBox';
import { ScrollBar } from './ScrollBar';
import { Static } from './Static';
import { TabCtrl } from './TabCtrl';
import { TextCtrl } from './TextCtrl';
import { TreeCtrl } from './TreeCtrl';

/** Creates the control object of a layout row (`layout`) or of a code-created control (`null`). */
export type ControlFactory = (kind: ControlKind, layout: ControlLayout | null) => Control | null;

/** The kinds that exist in a layout (`WTYPE_CUSTOM` creates nothing; `WTYPE_BASE` never occurs). */
export const defaultControlFactory: ControlFactory = (kind, layout) => {
  if (kind === WTYPE_CUSTOM) return new CustomCtrl(layout);
  if (kind === WTYPE_BUTTON) return new Button(layout);
  if (kind === WTYPE_EDITCTRL) return new EditCtrl(layout);
  if (kind === WTYPE_STATIC) return new Static(layout);
  if (kind === WTYPE_TEXT) return new TextCtrl(layout);
  if (kind === WTYPE_SCROLLBAR) return new ScrollBar();
  if (kind === WTYPE_LISTBOX) return new ListBox(layout);
  if (kind === WTYPE_COMBOBOX) return new ComboBox(layout);
  if (kind === WTYPE_TABCTRL) return new TabCtrl(layout);
  if (kind === WTYPE_TREECTRL) return new TreeCtrl(layout);
  const c = new Control(kind, layout);
  c.view = GenericControlView;
  return c;
};
