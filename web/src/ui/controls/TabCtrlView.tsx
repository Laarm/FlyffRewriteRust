// The pixels of a tab control (`CWndTabCtrl::OnDraw`; Plan/42 §7.6): the selected tab drawn from `WndTabTile10-15`
// over the baked strip, the captions (white unselected, black selected), and the pages.

import { useSyncExternalStore } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { FONT, fontStyle, lineHeight, textMetricsEpoch, textWidth } from '../text/fonts';
import { PageHost } from '../wnd/FrameWindowView';
import type { TabCtrl } from './TabCtrl';
import { selectedTabTiles, tabCaptionLen, tabCaptionX, tabCaptionY, TAB_TILE_H } from './tabMath';
import type { Sprite, SpriteState } from './sprites';
import { useSprite } from './useSprite';

const ready = (s: SpriteState): s is Sprite => typeof s === 'object';

export function TabCtrlView({ control }: { control: TabCtrl }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  useSyncExternalStore(textMetricsEpoch.subscribe, textMetricsEpoch.get);
  const env = control.env;
  const t10 = useSprite(env, 'WndTabTile10.bmp');
  const t11 = useSprite(env, 'WndTabTile11.bmp');
  const t12 = useSprite(env, 'WndTabTile12.bmp');
  const t13 = useSprite(env, 'WndTabTile13.bmp');
  const t14 = useSprite(env, 'WndTabTile14.bmp');
  const t15 = useSprite(env, 'WndTabTile15.bmp');
  const tiles = [t10, t11, t12, t13, t14, t15];
  const w = rectWidth(control.rectWindow);
  const h = rectHeight(control.rectWindow);
  const n = s.tabs.length;
  const len = tabCaptionLen(w, n);
  const y = tabCaptionY(h);
  return (
    <div className="tab" data-testid="tabctrl" role="tablist" style={{ width: w, height: h }}>
      {n > 0 &&
        selectedTabTiles(w, h, n, s.cur).map((t, i) => {
          const sp = tiles[t.tile];
          if (!sp || !ready(sp)) return null;
          return (
            <div
              key={i}
              className="tab-tile"
              data-tile={t.tile}
              style={{
                left: t.x,
                top: t.y,
                width: t.w,
                height: TAB_TILE_H,
                backgroundImage: `url("${sp.url}")`,
                backgroundSize: `${String(t.w)}px ${String(TAB_TILE_H)}px`,
              }}
            />
          );
        })}
      {s.tabs.map((tab, i) => (
        <div
          key={i}
          className="tab-caption"
          role="tab"
          aria-selected={i === s.cur}
          data-selected={i === s.cur}
          style={{
            left: tabCaptionX(i, len, textWidth(FONT.text, tab.text), s.align),
            top: y,
            height: lineHeight(FONT.text),
            color: i === s.cur ? '#000' : '#fff',
            ...fontStyle(FONT.text),
          }}
        >
          {tab.text}
        </div>
      ))}
      {s.tabs.map((tab, i) =>
        tab.page ? <PageHost key={tab.page.uid} page={tab.page} /> : <span key={`n${String(i)}`} />,
      )}
    </div>
  );
}
