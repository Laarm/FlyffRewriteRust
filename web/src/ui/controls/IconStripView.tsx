// The pixels of an icon strip: one absolutely positioned picture per entry (`useSprite` loads the file once and shares it), the
// flash animation, the multiply tint and the timer texts in the entry's font.

import { useSyncExternalStore } from 'react';
import { FONT, fontStyle, textMetricsEpoch } from '../text/fonts';
import { cssColor } from '../text/markup';
import type { WndEnv } from '../wnd/env';
import type { IconStrip, StripEntry } from './IconStrip';
import { useSprite } from './useSprite';

function EntryImage({ env, entry }: { env: WndEnv; entry: StripEntry }) {
  const sprite = useSprite(env, entry.icon);
  const url = typeof sprite === 'object' ? sprite.url : null;
  const tint = entry.tint ?? null;
  const style: React.CSSProperties = {
    position: 'absolute',
    left: 0,
    top: 0,
    width: entry.w,
    height: entry.h,
    backgroundRepeat: 'no-repeat',
    backgroundSize: `${String(entry.w)}px ${String(entry.h)}px`,
    opacity: entry.flash === true ? undefined : entry.alpha / 255,
    pointerEvents: 'none',
  };
  if (url) {
    style.backgroundImage = `url("${url}")`;
    if (tint) {
      style.backgroundColor = `rgb(${String(tint[0])}, ${String(tint[1])}, ${String(tint[2])})`;
      style.backgroundBlendMode = 'multiply';
      style.maskImage = `url("${url}")`;
      style.maskSize = `${String(entry.w)}px ${String(entry.h)}px`;
      style.maskRepeat = 'no-repeat';
    }
  }
  return (
    <div
      className={entry.flash === true ? 'strip-img ui-strip-flash' : 'strip-img'}
      data-testid="strip-img"
      data-flash={entry.flash === true}
      data-tint={tint ? tint.join(',') : undefined}
      style={style}
    />
  );
}

export function IconStripView({ control }: { control: IconStrip }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  useSyncExternalStore(textMetricsEpoch.subscribe, textMetricsEpoch.get);
  return (
    <div className="strip" data-testid="icon-strip">
      {s.entries.map((e) => (
        <div
          key={e.key}
          className="strip-entry"
          data-testid="strip-entry"
          data-key={e.key}
          style={{ left: e.x, top: e.y, width: e.w, height: e.h }}
        >
          {e.icon !== null && <EntryImage env={control.env} entry={e} />}
          {(e.texts ?? []).map((t, i) => (
            <div
              // the texts of an entry are a fixed short list
              key={i}
              className="strip-text"
              style={{
                left: t.x - e.x,
                top: t.y - e.y,
                color: cssColor(t.color),
                ...fontStyle(t.font === 'title' ? FONT.title : FONT.text),
              }}
            >
              {t.text}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
