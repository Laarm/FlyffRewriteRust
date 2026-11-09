// The texts of the pet status window as pure functions (`CWndPetStatus::DrawPetInformation` and the numbers of `OnDraw`, `src:_Interface/WndPetSys.cpp:382-621`;
// WP-UI-33): the positions are the original's `TextOut` arguments, including its approximations of the text width (`nChar * 5.8`, `nChar * 2.6`).

import type { TextItem } from '../../controls/TextLayer';
import type { Rect } from '../../layout/geometry';
import { expPercentText, percentOf } from '../char/statusText';
import type { PetStatusLabels, PetStatusView } from './ports';

/** `D3DCOLOR_ARGB(255, 74, 74, 173)` and its shadow `D3DCOLOR_ARGB(255, 160, 160, 240)`: the labels. */
export const LABEL_COLOR = 0xff4a4aad;
export const LABEL_SHADOW = 0xffa0a0f0;
/** `D3DCOLOR_ARGB(255, 255, 28, 174)`: the values of the information block. */
export const VALUE_COLOR = 0xffff1cae;
/** White: the numbers over the gauges. */
export const NUMBER_COLOR = 0xffffffff;

/** `m_nDisplay`: 0 nothing, 1 percent and value, 2 value / maximum. Initial 2; a click goes 0 after 2, then 1, 2 (`++m_nDisplay > 2 ? 0`). */
export type PetDisplay = 0 | 1 | 2;
export const PET_DISPLAY_INITIAL: PetDisplay = 2;
export function nextPetDisplay(d: PetDisplay): PetDisplay {
  return d === 2 ? 0 : d === 0 ? 1 : 2;
}

/** The rects of the three custom areas the texts hang on, in client coordinates. */
export interface PetStatusRects {
  readonly hp: Rect;
  readonly exp: Rect;
}

/** The pixel width the original assumes for the label "Ability" when it places the value after it (`GetTextExtent`); supplied by the caller. */
export interface PetTextMetrics {
  /** `GetTextExtent( TID_GAME_PET_STATUS_ABILITY ).cx` in the text font. */
  readonly abilityLabelWidth: number;
}

/** `DrawPetInformation` + the numbers: every text of the window for a view and a display mode. */
export function petStatusTexts(o: {
  readonly view: PetStatusView;
  readonly labels: PetStatusLabels;
  readonly display: PetDisplay;
  readonly rects: PetStatusRects;
  readonly metrics: PetTextMetrics;
}): TextItem[] {
  const { view, labels, display, rects } = o;
  const items: TextItem[] = [];
  const label = (x: number, y: number, text: string): void => {
    items.push({ x: x + 1, y: y + 1, text, color: LABEL_SHADOW });
    items.push({ x, y, text, color: LABEL_COLOR });
  };
  const noLife = view.life === null;
  label(74, 4, labels.level);
  label(74, 18, labels.ability);
  if (!noLife) label(74, 46, labels.hp);
  label(74, 61, labels.exp);
  items.push({ x: 112, y: 4, text: view.grade, color: VALUE_COLOR });
  items.push({
    x: 76 + o.metrics.abilityLabelWidth,
    y: 18,
    text: view.ability,
    color: VALUE_COLOR,
  });
  if (!noLife) items.push({ x: 100, y: 32, text: String(view.life), color: VALUE_COLOR });

  if (display === 0) return items;
  const put = (x: number, y: number, text: string): void => {
    items.push({ x, y, text, color: NUMBER_COLOR, font: 'title' });
  };
  const hp = rects.hp;
  if (!noLife) {
    if (display === 1) {
      put(hp.r - 70, hp.t, `${String(percentOf(view.energy, view.energyMax))}%`);
      const t = `${String(view.energy)}p`;
      put(hp.r - 7 - Math.trunc(t.length * 5.8), hp.t, t);
    } else {
      const cur = String(view.energy);
      put(hp.r - 76 + 15 - Math.trunc(cur.length * 2.6), hp.t, cur);
      const max = String(view.energyMax);
      put(hp.r - 38 + 15 - max.length * 2, hp.t, max);
      put(hp.r - 42, hp.t, '/');
    }
  }
  // `nExpResult = exp * 10000 / maxExp` in 64-bit integers, then `fExp = nExpResult / 100`
  const x100 = view.expMax > 0 ? Math.trunc((view.exp * 10000) / view.expMax) : 0;
  const exp = expPercentText(x100);
  put(hp.r - 7 - Math.trunc(exp.length * 5.8), rects.exp.t, exp);
  return items;
}
