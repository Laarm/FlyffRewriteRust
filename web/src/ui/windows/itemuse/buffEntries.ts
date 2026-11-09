// The picture of a buff icon (`CWndWorld::RenderBuffIcon`, `CWndBuffStatus::RenderBuffIcon`, ref/22 §17 C): the world strip draws the
// 32 x 32 picture as it is, the status window at scale 1.2 (38 x 38); item and equip buffs have alpha 192, flashing buffs pulse
// between 64 and 192; an evil buff is multiplied by (255, 120, 255). The timer text sits under the picture.

import type { StripEntry } from '../../controls/IconStrip';
import type { TextItem } from '../../controls/TextLayer';
import type { BuffIcon } from './ports';

/** `1.2f` of a 32 x 32 picture (the status window). */
export const BUFF_ICON_SIZE = 38;
/** `rectHittest` of the tooltip in the status window. */
export const BUFF_TIP_SIZE = 28;
/** The multiply colour of an evildoing buff. */
export const EVIL_TINT = [255, 120, 255] as const;

/** The strip entries of one icon at (x, y): the picture, its half-size copies and the timer text. */
export function buffEntry(icon: BuffIcon, x: number, y: number): StripEntry[] {
  const texts: TextItem[] = [];
  if (icon.timer !== '') {
    const tx = x + icon.timerDx;
    const ty = y + icon.timerDy;
    if (icon.timerShadow)
      texts.push({
        x: tx + 1,
        y: ty + 1,
        text: icon.timer,
        color: 0xff000000,
        font: icon.timerFont,
      });
    texts.push({ x: tx, y: ty, text: icon.timer, color: icon.timerColor, font: icon.timerFont });
  }
  const tint = icon.evil ? EVIL_TINT : null;
  const out: StripEntry[] = [
    {
      key: icon.key,
      icon: icon.icon,
      x,
      y,
      w: icon.size,
      h: icon.size,
      alpha: icon.alpha,
      flash: icon.flash,
      tint,
      texts,
    },
  ];
  (icon.extras ?? []).forEach((e, i) => {
    out.push({
      key: `${icon.key}#${String(i + 1)}`,
      icon: icon.icon,
      x: x + e.dx,
      y: y + e.dy,
      w: e.size,
      h: e.size,
      alpha: icon.alpha,
      tint,
    });
  });
  return out;
}
