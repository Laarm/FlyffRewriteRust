// The placeholder view of every control kind until its work package (WP-UI-05..07) lands: it shows the caption of
// buttons, statics and texts in the original colours and nothing else, so a window built from a layout is readable
// in the gallery. The positioned host element (`.ctrl`, rect, visibility) is rendered by `FrameWindowView`.

import { useSyncExternalStore } from 'react';
import { WTYPE_BUTTON, WTYPE_STATIC, WTYPE_TEXT } from '../layout/styles';
import type { ControlViewProps } from '../wnd/Control';

/** Caption colour of a static (`0xFF2E70A9`) and of a push button (`rgb(64,64,64)`). */
const STATIC_COLOR = '#2e70a9';
const BUTTON_COLOR = 'rgb(64,64,64)';

export function GenericControlView({ control }: ControlViewProps) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const showText =
    control.kind === WTYPE_STATIC || control.kind === WTYPE_BUTTON || control.kind === WTYPE_TEXT;
  const color = control.kind === WTYPE_STATIC ? STATIC_COLOR : BUTTON_COLOR;
  return (
    <div
      className="ctrl-generic"
      style={{ color, paddingLeft: control.kind === WTYPE_STATIC ? 2 : 0 }}
    >
      {showText ? s.text : null}
    </div>
  );
}
