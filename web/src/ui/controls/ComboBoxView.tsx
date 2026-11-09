// The combo: the selected text at the client origin (`CWndEdit::OnDraw`, black, FONT.text) and the drop button.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, fontStyle } from '../text/fonts';
import { ControlHost } from '../wnd/FrameWindowView';
import type { ComboBox } from './ComboBox';

export function ComboBoxView({ control }: { control: ComboBox }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const button = control.dropButton;
  return (
    <div
      className="cb"
      data-testid="combobox"
      role="combobox"
      aria-expanded={s.open}
      aria-haspopup="listbox"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      <div className="cb-field" />
      <div
        className="cb-text"
        style={{
          left: c.l - w.l,
          top: c.t - w.t,
          width: Math.max(0, rectWidth(c)),
          ...fontStyle(FONT.text),
        }}
      >
        {s.display}
      </div>
      {button && <ControlHost control={button} />}
    </div>
  );
}
