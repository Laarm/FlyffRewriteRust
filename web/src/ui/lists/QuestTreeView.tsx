// The quest tree: the rows of the tree (open / close icon, title in the state colour) plus a check box on every quest leaf.

import { useSyncExternalStore } from 'react';
import { useSprite } from '../controls/useSprite';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, lineHeight } from '../text/fonts';
import type { RichLine } from '../text/layout';
import { RichText } from '../text/RichText';
import { ControlHost } from '../wnd/FrameWindowView';
import type { QuestTree } from './QuestTree';
import { CHECK_SIZE } from './QuestTree';

export function QuestTreeView({ control }: { control: QuestTree }) {
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
      data-testid="quest-tree"
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
          const box = control.hasCheckBox(n) ? control.checkRect(r.icon) : null;
          return (
            <div
              key={n.id}
              role="treeitem"
              data-state={control.stateOf(n)}
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
              {box && (
                <div
                  data-testid="quest-check"
                  data-checked={control.isChecked(n)}
                  role="checkbox"
                  aria-checked={control.isChecked(n)}
                  style={{
                    position: 'absolute',
                    left: box.l,
                    top: box.t,
                    width: CHECK_SIZE,
                    height: CHECK_SIZE,
                    boxSizing: 'border-box',
                    border: '1px solid #404040',
                    background: '#fff',
                    color: '#c00000',
                    fontSize: 11,
                    lineHeight: `${String(CHECK_SIZE - 2)}px`,
                    textAlign: 'center',
                    pointerEvents: 'none',
                  }}
                >
                  {control.isChecked(n) ? '✓' : ''}
                </div>
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
