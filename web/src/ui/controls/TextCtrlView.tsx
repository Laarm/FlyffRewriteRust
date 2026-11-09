// The pixels of a text control (Plan/42 §8.6): the visible lines of the laid-out rich text in the client rect, plus
// the docking scroll bar. The 9-tile skin behind it is baked into the frame.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { RichText } from '../text/RichText';
import { ControlHost } from '../wnd/FrameWindowView';
import type { TextCtrl } from './TextCtrl';

export function TextCtrlView({ control }: { control: TextCtrl }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const bar = control.scrollBar;
  return (
    <div className="tx" data-testid="text" style={{ width: rectWidth(w), height: rectHeight(w) }}>
      <div
        className={s.clipboard ? 'tx-client tx-select' : 'tx-client'}
        style={{
          left: c.l - w.l,
          top: c.t - w.t,
          width: Math.max(0, rectWidth(c)),
          height: Math.max(0, rectHeight(c)),
          ...(s.alpha === 255 ? {} : { opacity: s.alpha / 255 }),
        }}
      >
        <RichText
          lines={s.lines}
          first={s.first}
          count={s.count}
          font={control.font}
          lineSpace={s.lineSpace}
        />
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
