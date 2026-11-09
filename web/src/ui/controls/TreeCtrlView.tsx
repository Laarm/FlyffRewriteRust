// The rows of a tree (`CWndTreeCtrl::PaintTree`): the open / close icon of every element with children, the keyword in
// the select colour when it is the selected element, the docking scroll bar.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, lineHeight } from '../text/fonts';
import type { RichLine } from '../text/layout';
import { RichText } from '../text/RichText';
import { ControlHost } from '../wnd/FrameWindowView';
import type { TreeCtrl } from './TreeCtrl';
import { useSprite } from './useSprite';

export function TreeCtrlView({ control }: { control: TreeCtrl }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const open = useSprite(control.env, 'buttTreeOpen.tga');
  const close = useSprite(control.env, 'buttTreeClose.tga');
  const w = control.rectWindow;
  const c = control.rectClient;
  const bar = control.scrollBar;
  const cell = lineHeight(FONT.text);
  const clientH = rectHeight(c);
  const rows = control.rows().filter((r) => r.text.t > -cell && r.text.t < clientH);
  return (
    <div
      className="tr"
      data-testid="tree"
      role="tree"
      style={{ width: rectWidth(w), height: rectHeight(w) }}
    >
      <div
        className="tr-client"
        data-version={s.version}
        style={{
          left: c.l - w.l,
          top: c.t - w.t,
          width: Math.max(0, rectWidth(c)),
          height: Math.max(0, clientH),
        }}
      >
        {rows.map((r) => {
          const n = r.node;
          const sprite = n.open ? open : close;
          const color = s.focus === n ? n.selectColor : n.color;
          const line: RichLine = {
            runs: n.runs.map((run) => ({ ...run, color })),
            width: 0,
            offset: 0,
            length: 0,
          };
          return (
            <div
              key={n.id}
              role="treeitem"
              aria-expanded={n.children.length > 0 ? n.open : undefined}
              aria-selected={s.focus === n}
            >
              {n.children.length > 0 && typeof sprite === 'object' && (
                <div
                  className="tr-icon"
                  style={{
                    left: r.icon.l,
                    top: r.icon.t,
                    width: sprite.width,
                    height: sprite.height,
                    backgroundImage: `url("${sprite.url}")`,
                  }}
                />
              )}
              <RichText
                lines={[line]}
                font={FONT.text}
                className="tr-text"
                style={{ left: r.text.l, top: r.text.t, height: cell }}
              />
            </div>
          );
        })}
      </div>
      {bar && <ControlHost control={bar} />}
    </div>
  );
}
