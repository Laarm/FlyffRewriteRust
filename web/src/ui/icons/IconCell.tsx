// `IconCell` (Plan/42 §8.10, §6.3, ref/22 §8 `CWndItemCtrl::RanderIcon/OnDraw`; WP-UI-12): one 32 x 32 icon of an item
// grid, an equipment slot or a confirmation window. It draws
//   * the icon (`Item/…` file) at its natural 32 x 32, `dim` at alpha 60/255 (item in use), `tint: 'red'` multiplied by
//     (255, 100, 100) (`Render2`: cannot be used / expired),
//   * the stack count of items with `packMax > 1`: `features.new_font_color` = `#EAEBDD` in the outlined title font at
//     (cell right − 2, bottom 34); off = the legacy blue pair `0xFF0000FF` at (right 32, bottom 32) plus `0xFFB0B0F0`
//     one pixel up and left (§8.10),
//   * the cooldown radar (`.flyff-cooldown`, a CSS animation started once per cooldown), the enchant glow
//     (`.flyff-enchant`) and the 4 px enlargement of the hovered cell (`m_nOnSelect`: 36 x 36 at (−2, −2)).
// Everything is CSS; nothing re-renders per frame.

import { memo, useState } from 'react';
import { useSprite } from '../controls/useSprite';
import { cooldownStyle } from './anim';
import { FONT, fontStyle, lineHeight } from '../text/fonts';
import type { WndEnv } from '../wnd/env';

/** What a cell shows. */
export interface IconCellData {
  /** The icon file (`item/itm_x.dds`, `icon/...`); `null` draws nothing (an item whose icon is missing). */
  readonly icon: string | null;
  /** The stack size; the number is drawn when `showCount` and `count > 0`. */
  readonly count?: number;
  /** `dwPackMax > 1`: the item shows its stack size (a count of 1 is shown as "1"). */
  readonly showCount?: boolean;
  /** Alpha 60/255: the item is being used (`IsUsingItem`). */
  readonly dim?: boolean;
  /** Multiplied by (255, 100, 100): the item cannot be used or is expired. */
  readonly tint?: 'red' | null;
  /** The cooldown group of the item: absolute `host.now()` ms of start and end. */
  readonly cooldown?: { readonly startMs: number; readonly endMs: number } | null;
  /** The upgrade-wait glow (`RenderEnchant`). */
  readonly enchant?: boolean;
}

export interface IconCellProps {
  readonly env: WndEnv;
  readonly cell: IconCellData;
  /** Left/top of the cell in its container. */
  readonly x: number;
  readonly y: number;
  /** `features.new_font_color`. */
  readonly newFontColor: boolean;
  /** The pointer is over the cell (`m_nOnSelect`): drawn 4 px larger. */
  readonly hover?: boolean;
  /** Test hook: `data-slot`. */
  readonly slot?: number;
}

/** `IsUsingItem` alpha. */
export const DIM_ALPHA = 60 / 255;
/** The red multiplier of `Render2(.., D3DCOLOR_XRGB(255, 100, 100))`. */
export const RED_TINT = 'rgb(255, 100, 100)';

/** The CSS of the icon image of a cell (also the equipment slots of the inventory): size, position, alpha and the red multiply. */
export function iconImageStyle(o: {
  readonly url: string | null;
  readonly size: number;
  /** The height when the icon is not square (the hero mark is 44 x 14). */
  readonly height?: number;
  readonly left: number;
  readonly top: number;
  readonly alpha: number;
  readonly tint: boolean;
}): React.CSSProperties {
  const h = o.height ?? o.size;
  const style: React.CSSProperties = {
    position: 'absolute',
    left: o.left,
    top: o.top,
    width: o.size,
    height: h,
    backgroundRepeat: 'no-repeat',
    backgroundSize: `${String(o.size)}px ${String(h)}px`,
    opacity: o.alpha,
  };
  if (o.url) {
    style.backgroundImage = `url("${o.url}")`;
    if (o.tint) {
      // texture * (255, 100, 100): multiply a flat colour into the image, keeping the image's own alpha
      style.backgroundColor = RED_TINT;
      style.backgroundBlendMode = 'multiply';
      style.maskImage = `url("${o.url}")`;
      style.maskSize = `${String(o.size)}px ${String(h)}px`;
      style.maskRepeat = 'no-repeat';
    }
  }
  return style;
}

/** The stack count: where and how it is drawn (a pure function of the flag, for the tests). */
export function countSpec(newFontColor: boolean): {
  readonly right: number;
  readonly top: number;
  readonly color: string;
  readonly shadow: { readonly right: number; readonly top: number; readonly color: string } | null;
} {
  if (newFontColor) {
    // TextOut(x + 32 - size.cx - 2, y + 34 - size.cy, ..., 0xFFEAEBDD) in the title font (outlined)
    return { right: 2, top: 34 - lineHeight(FONT.title), color: '#EAEBDD', shadow: null };
  }
  // TextOut(x + 32 - cx, y + 32 - cy, 0xFF0000FF); TextOut(x + 31 - cx, y + 31 - cy, 0xFFB0B0F0)
  const cell = lineHeight(FONT.text);
  return {
    right: 1,
    top: 31 - cell,
    color: '#B0B0F0',
    shadow: { right: 0, top: 32 - cell, color: '#0000FF' },
  };
}

function Cooldown({ startMs, endMs, now }: { startMs: number; endMs: number; now: () => number }) {
  const [done, setDone] = useState(false);
  // the animation is anchored once per cooldown: the parent keys this component by (start, end)
  const [elapsed] = useState(() => now() - startMs);
  const total = endMs - startMs;
  if (done || total <= 0) return null;
  return (
    <div
      className="flyff-cooldown"
      data-testid="cooldown"
      style={cooldownStyle(total, elapsed)}
      onAnimationEnd={() => {
        setDone(true);
      }}
    />
  );
}

export const IconCell = memo(function IconCell({
  env,
  cell,
  x,
  y,
  newFontColor,
  hover = false,
  slot,
}: IconCellProps) {
  const sprite = useSprite(env, cell.icon);
  const url = typeof sprite === 'object' ? sprite.url : null;
  const size = hover ? 36 : 32;
  const off = hover ? -2 : 0;
  const tinted = cell.tint === 'red';
  const iconStyle = iconImageStyle({
    url,
    size,
    left: off,
    top: off,
    alpha: cell.dim === true ? DIM_ALPHA : 1,
    tint: tinted,
  });
  const showCount = cell.showCount === true && (cell.count ?? 0) > 0;
  const spec = countSpec(newFontColor);
  return (
    <div
      className="icon-cell"
      data-testid="icon-cell"
      data-slot={slot}
      data-hover={hover}
      data-dim={cell.dim === true}
      data-tint={tinted ? 'red' : undefined}
      style={{ left: x, top: y, width: 32, height: 32 }}
    >
      <div className="icon-img" data-testid="icon-img" style={iconStyle} />
      {showCount && (
        <>
          {spec.shadow && (
            <span
              className="icon-count"
              data-testid="icon-count-shadow"
              style={{
                ...fontStyle(FONT.text),
                right: spec.shadow.right,
                top: spec.shadow.top,
                color: spec.shadow.color,
              }}
            >
              {cell.count}
            </span>
          )}
          <span
            className="icon-count"
            data-testid="icon-count"
            data-count-style={newFontColor ? 'new' : 'legacy'}
            style={{
              ...fontStyle(newFontColor ? FONT.title : FONT.text),
              right: spec.right,
              top: spec.top,
              color: spec.color,
            }}
          >
            {cell.count}
          </span>
        </>
      )}
      {cell.cooldown && (
        <Cooldown
          key={`${String(cell.cooldown.startMs)}:${String(cell.cooldown.endMs)}`}
          startMs={cell.cooldown.startMs}
          endMs={cell.cooldown.endMs}
          now={() => env.host.now()}
        />
      )}
      {cell.enchant === true && <div className="flyff-enchant" data-testid="enchant" />}
    </div>
  );
});
