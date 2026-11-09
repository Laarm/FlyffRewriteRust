// The pixels of a button (`CTheme::RenderWndButton*`, ITheme.cpp:1259-1609; Plan/42 §8.5): the frame of the sprite strip
// for the current state, the caption where the original puts it, the untextured fallbacks (gradient push button, vector
// check box and radio), the menu item look. A strip that has not loaded yet draws nothing (no flash of the fallback).

import { useSyncExternalStore, type CSSProperties } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { WBS_CHECK, WBS_MENUITEM, WBS_NOCENTER, WBS_RADIO, WBS_TEXT } from '../layout/styles';
import { FONT, fontStyle, lineHeight, textMetricsEpoch, textWidth } from '../text/fonts';
import { cssColor } from '../text/markup';
import { type Button, buttonFontColor, frame4, frame6 } from './Button';
import { type Sprite, frameBackground } from './sprites';
import { useSprite } from './useSprite';

const text = FONT.text;

/** The caption position of `GetStrCenter`: centred in the window rect with integer division. */
function centred(w: number, h: number, label: string): { x: number; y: number } {
  return {
    x: Math.trunc(w / 2) - Math.trunc(textWidth(text, label) / 2),
    y: Math.trunc(h / 2) - Math.trunc(lineHeight(text) / 2),
  };
}

function Caption({
  x,
  y,
  color,
  label,
  shadow,
}: {
  x: number;
  y: number;
  color: number;
  label: string;
  shadow?: boolean;
}) {
  const style: CSSProperties = { left: x, top: y, color: cssColor(color), ...fontStyle(text) };
  if (shadow) style.textShadow = '1px 1px 0 #000';
  return (
    <div className="bt-text" style={style}>
      {label}
    </div>
  );
}

export function ButtonView({ control }: { control: Button }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  useSyncExternalStore(textMetricsEpoch.subscribe, textMetricsEpoch.get);
  const sprite = useSprite(control.env, s.texture);
  const icon = useSprite(control.env, s.icon);
  const w = rectWidth(s.rect);
  const h = rectHeight(s.rect);
  const highlight = control.isHighLight();
  const push = s.pushed;
  const color = buttonFontColor(s, highlight);
  const radio = (s.style & WBS_RADIO) !== 0;
  const check = (s.style & WBS_CHECK) !== 0;
  const kind = radio
    ? 'radio'
    : check
      ? 'check'
      : (s.style & WBS_MENUITEM) !== 0
        ? 'menuitem'
        : (s.style & WBS_TEXT) !== 0
          ? 'text'
          : 'push';
  const loading = sprite === 'loading';
  const tex: Sprite | null = typeof sprite === 'object' ? sprite : null;
  const aria = {
    role: radio ? 'radio' : check ? 'checkbox' : 'button',
    'aria-label': s.text === '' ? undefined : s.text,
    'aria-disabled': !s.enabled,
    ...(radio || check ? { 'aria-checked': s.checked } : {}),
  } as const;
  const common = {
    'data-testid': 'button',
    'data-kind': kind,
    'data-hover': highlight,
    'data-pushed': push,
    'data-checked': s.checked,
  };

  if (kind === 'menuitem')
    return (
      <MenuItemLook
        s={s}
        w={w}
        h={h}
        icon={typeof icon === 'object' ? icon : null}
        common={common}
        aria={aria}
      />
    );

  // ---- textured --------------------------------------------------------------------------------------------
  if (tex || loading) {
    if (!tex) return <div className="bt" {...common} {...aria} style={{ width: w, height: h }} />;
    if (kind === 'radio' || kind === 'check') {
      const frame = frame6(s.enabled, push, s.checked, highlight);
      const fw = Math.floor(tex.width / 6);
      return (
        <div
          className="bt"
          {...common}
          {...aria}
          data-frame={frame}
          style={{ width: w, height: h }}
        >
          <div
            className="bt-sprite"
            style={{ width: fw, height: tex.height, ...frameBackground(tex, 6, frame) }}
          />
          <Caption
            x={fw + 2}
            y={Math.trunc(h / 2) - Math.trunc(lineHeight(text) / 2)}
            color={color}
            label={s.text}
          />
        </div>
      );
    }
    const frame = frame4(s.enabled, push, s.checked, highlight);
    const fw = Math.floor(tex.width / 4);
    return (
      <div className="bt" {...common} {...aria} data-frame={frame} style={{ width: w, height: h }}>
        <div
          className="bt-sprite"
          style={{ width: fw, height: tex.height, ...frameBackground(tex, 4, frame) }}
        />
      </div>
    );
  }

  // ---- untextured (no texture, or a missing file: KQ-30) ------------------------------------------------------
  if (kind === 'radio') {
    return (
      <div className="bt" {...common} {...aria} style={{ width: w, height: h }}>
        <div className="bt-radio" style={{ borderColor: cssColor(color) }}>
          {s.checked && <div className="bt-radio-dot" style={{ background: cssColor(color) }} />}
        </div>
        <Caption x={lineHeight(text) + 5} y={0} color={color} label={s.text} />
      </div>
    );
  }
  if (kind === 'check') {
    return (
      <div className="bt" {...common} {...aria} style={{ width: w, height: h }}>
        <div className="bt-check" style={{ borderColor: cssColor(color) }}>
          {s.checked && (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path
                d="M2 2 L5 8 M2 2 L6 8 M5 8 L8 3"
                stroke={cssColor(color)}
                strokeWidth="1"
                fill="none"
              />
            </svg>
          )}
        </div>
        <Caption x={lineHeight(text) + 5} y={0} color={color} label={s.text} />
      </div>
    );
  }
  const pos = (s.style & WBS_NOCENTER) !== 0 ? { x: 0, y: 0 } : centred(w, h, s.text);
  if (kind === 'text') {
    return (
      <div className="bt" {...common} {...aria} style={{ width: w, height: h }}>
        <Caption x={pos.x} y={pos.y} color={color} label={s.text} />
      </div>
    );
  }
  // the standard untextured push button: the caption with a 1 px black shadow, then the translucent gradient over it
  const gradient = push
    ? 'linear-gradient(rgba(100,255,255,0.996), rgba(0,0,0,0.196) 40%, rgba(0,150,150,0.784))'
    : 'linear-gradient(rgba(255,255,255,0.98), rgba(0,0,0,0.196) 40%, rgba(150,150,150,0.784))';
  return (
    <div className="bt" {...common} {...aria} style={{ width: w, height: h }}>
      <Caption x={pos.x + 1} y={pos.y + 1} color={0xff000000} label={s.text} />
      <Caption x={pos.x} y={pos.y} color={s.checked ? 0 : color} label={s.text} />
      <div
        className="bt-face"
        style={{ left: 2, top: 2, right: 2, bottom: 2, background: gradient }}
      />
    </div>
  );
}

function MenuItemLook({
  s,
  w,
  h,
  icon,
  common,
  aria,
}: {
  s: ReturnType<Button['state']['get']>;
  w: number;
  h: number;
  icon: Sprite | null;
  common: Record<string, unknown>;
  aria: Record<string, unknown>;
}) {
  const hl = common['data-hover'] === true;
  const fill = hl || s.menuOpen ? 'rgba(200,200,200,0.608)' : 'transparent';
  return (
    <div
      className="bt bt-menuitem"
      {...common}
      {...aria}
      role="menuitem"
      style={{ width: w, height: h, background: fill }}
    >
      {icon && (
        <div
          className="bt-icon"
          style={{
            width: 20,
            height: 20,
            backgroundImage: `url("${icon.url}")`,
            backgroundSize: '20px 20px',
          }}
        />
      )}
      {s.checked && !s.hasMenu && (
        <svg className="bt-tick" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2 2 L5 8 M2 2 L6 8 M5 8 L8 3" stroke="#000" strokeWidth="1" fill="none" />
        </svg>
      )}
      <Caption x={24} y={4} color={0xff000000} label={s.text} />
      {s.hotkey !== '' && !s.hasMenu && (
        <Caption x={w - 32} y={4} color={0xff000000} label={s.hotkey} />
      )}
      {s.hasMenu && (
        <svg
          className="bt-arrow"
          width="6"
          height={Math.max(0, h - 8)}
          viewBox={`0 0 6 ${String(Math.max(1, h - 8))}`}
          style={{ left: w - 10, top: 4 }}
          aria-hidden="true"
        >
          <path
            d={`M0 0 L0 ${String(Math.max(1, h - 8))} L5 ${String(Math.max(1, h - 8) / 2)} Z`}
            fill="#a0a0ff"
          />
        </svg>
      )}
    </div>
  );
}
