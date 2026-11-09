// The font table of the UI (Plan/42 §8.1, ref/22 §6.2; PL-126): the `FONT.*` slots, their CSS (stack, outline,
// faux bold), the fixed Verdana line metrics (`FONT_CELL`) and the text measurement the line breaker, the aligned
// statics, the tooltips and the lists share (`textWidth`, cached per font and character).
//
// Every face is Verdana in the original (ENG). Linux browsers have no Verdana, so every stack ends in the bundled
// DejaVu Sans (`FALLBACK_FONT_FAMILY`, declared once in `styles/fonts.css`), and the LINE METRICS are fixed to
// Verdana's GDI cell heights so layouts do not depend on the fallback font.

import type { CSSProperties } from 'react';
import { Observable } from '../wnd/observable';

/** The one family-name constant of the bundled fallback font (also awaited by the boot, Plan/40 §3.1 B1c). */
export const FALLBACK_FONT_FAMILY = 'Flyff DejaVu Sans';

/** One `FONT.*` slot. `outlinePx` of 0 means no outline; `px = MulDiv(pt, 96, 72)`. */
export interface FontSpec {
  readonly px: number;
  readonly outlinePx: number;
  /** CSS colour of the 8-direction outline (`d3dfont.cpp:333-356`). */
  readonly outlineColor: string | null;
  readonly bold: boolean;
}

/** The table of §8.1 (`FONT.text`, `status`, `title`, `world`, `caption`, `guildCombat`, `secretRoom`). */
export const FONT = {
  /** `m_pFontText` (gulim9), also the chat font: every window's default. */
  text: { px: 12, outlinePx: 0, outlineColor: null, bold: false },
  /** `m_pFontStatus` (gulim8). */
  status: { px: 11, outlinePx: 0, outlineColor: null, bold: false },
  /** `m_pFontWndTitle` (Arial Black9 key): window titles, item counts with `__NEW_FONT_COLOR`. */
  title: { px: 12, outlinePx: 2, outlineColor: 'rgb(217,91,51)', bold: false },
  /** `m_pFontWorld`: names in the world, centre messages. */
  world: { px: 12, outlinePx: 2, outlineColor: 'rgb(60,60,60)', bold: false },
  /** `m_pFontCaption` (gulim20, filtered). */
  caption: { px: 20, outlinePx: 2, outlineColor: 'rgb(40,100,220)', bold: false },
  /** `m_pFontGuildCombatText` (gulim13). */
  guildCombat: { px: 17, outlinePx: 0, outlineColor: null, bold: false },
  /** Secret-room guild text (gulim11 bold, outline 1). */
  secretRoom: { px: 11, outlinePx: 1, outlineColor: 'rgb(0,0,0)', bold: true },
} as const satisfies Record<string, FontSpec>;

export type FontKey = keyof typeof FONT;

/**
 * Verdana's GDI line cell height per pixel size (`round(px*2059/2048) + round(px*430/2048)`, usWinAscent and
 * usWinDescent): 9 -> 11, 11 -> 13, 12 -> 15, 17 -> 21, 20 -> 24. An outlined font adds `2 * outline`.
 */
export function fontCell(px: number): number {
  return Math.round((px * 2059) / 2048) + Math.round((px * 430) / 2048);
}

/** `FONT_CELL` of Plan/42 §8.1: Verdana's GDI cell height per pixel size (the sizes of the `FONT.*` table). */
export const FONT_CELL: Readonly<Record<number, number>> = {
  9: 11,
  11: 13,
  12: 15,
  17: 21,
  20: 24,
};

/** The line height of a font slot in px (outline included, as `GetMaxHeight` returns it). */
export function lineHeight(font: FontSpec): number {
  return fontCell(font.px) + 2 * font.outlinePx;
}

let fallbackFirstFlag = false;

/**
 * `font=fallback` of the gallery (Plan/42 §21.1): forces `FALLBACK_FONT_FAMILY` first in every stack, so the CI
 * screenshots do not depend on the fonts of the machine. Changing it invalidates the measurements.
 */
export function setFontFallbackFirst(on: boolean): void {
  if (fallbackFirstFlag === on) return;
  fallbackFirstFlag = on;
  invalidateTextMetrics();
}

/** Whether the fallback family is forced first (`font=fallback`). */
export function isFontFallbackFirst(): boolean {
  return fallbackFirstFlag;
}

/** `font-family` of every UI text: Verdana first, the bundled fallback second; `fallbackFirst` forces CI determinism. */
export function fontFamilyStack(fallbackFirst = fallbackFirstFlag): string {
  return fallbackFirst
    ? `"${FALLBACK_FONT_FAMILY}", Verdana, sans-serif`
    : `Verdana, "${FALLBACK_FONT_FAMILY}", sans-serif`;
}

/**
 * The outline as `text-shadow`: the glyph drawn 8 times offset by `(±o,±o), (0,±o), (±o,0)` in the outline
 * colour, blur 0 (Plan/42 §8.1).
 */
export function outlineShadow(outlinePx: number, color: string): string {
  const o = outlinePx;
  const offsets: [number, number][] = [
    [-o, -o],
    [0, -o],
    [o, -o],
    [-o, 0],
    [o, 0],
    [-o, o],
    [0, o],
    [o, o],
  ];
  return offsets.map(([x, y]) => `${String(x)}px ${String(y)}px 0 ${color}`).join(', ');
}

/** Faux bold of statics and group boxes (drawn at x and x + 1). */
export const FAUX_BOLD_SHADOW = '1px 0 0 currentColor';

/** The CSS of a font slot (family stack, size, line height, outline, no synthesis or kerning). */
export function fontStyle(font: FontSpec, fallbackFirst = fallbackFirstFlag): CSSProperties {
  const style: CSSProperties = {
    fontFamily: fontFamilyStack(fallbackFirst),
    fontSize: `${String(font.px)}px`,
    lineHeight: `${String(lineHeight(font))}px`,
    fontWeight: font.bold ? 700 : 400,
    fontSynthesis: 'none',
    fontKerning: 'none',
  };
  if (font.outlinePx > 0 && font.outlineColor) {
    style.textShadow = outlineShadow(font.outlinePx, font.outlineColor);
  }
  return style;
}

// ---- measurement (Plan/42 §8.1: widths are measured on the font that is actually loaded) ---------------------------

/** Measures the advance of `text` in a CSS font (`ctx.font` syntax), in px. */
export type TextMeasurer = (cssFont: string, text: string) => number;

/** The `ctx.font` string of a font slot (`bold 11px "Flyff DejaVu Sans", Verdana, sans-serif`). */
export function cssFont(font: FontSpec, fallbackFirst = fallbackFirstFlag): string {
  return `${font.bold ? 'bold ' : ''}${String(font.px)}px ${fontFamilyStack(fallbackFirst)}`;
}

let canvas2d: { font: string; measureText(text: string): { width: number } } | null | undefined;

/** The canvas 2-D measurer; `null` when the environment has no canvas (Node tests, `happy-dom`). */
function canvasMeasurer(): TextMeasurer | null {
  if (canvas2d === undefined) {
    canvas2d = null;
    try {
      if (typeof OffscreenCanvas === 'function') {
        canvas2d = new OffscreenCanvas(1, 1).getContext('2d');
      } else if (typeof document !== 'undefined') {
        canvas2d = document.createElement('canvas').getContext('2d');
      }
    } catch {
      canvas2d = null;
    }
  }
  const ctx = canvas2d;
  if (!ctx) return null;
  return (font, text) => {
    ctx.font = font;
    return ctx.measureText(text).width;
  };
}

/** The measurer used without a canvas: a deterministic proportional guess (0.6 em per character). */
export const approximateMeasurer: TextMeasurer = (font, text) => {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  const px = m ? Number(m[1]) : 12;
  return Array.from(text).length * px * 0.6;
};

/** A measurer with the same advance for every character (tests that need exact widths). */
export function fixedWidthMeasurer(width: number): TextMeasurer {
  return (_font, text) => Array.from(text).length * width;
}

let measurerOverride: TextMeasurer | null = null;
const charCache = new Map<string, number>();

/** Observable counter that changes whenever the metrics were invalidated (a font finished loading, `font=fallback`). */
export const textMetricsEpoch = new Observable<number>(0);

/** Forgets every cached width and tells the views that measure text (fonts finished loading, the measurer changed). */
export function invalidateTextMetrics(): void {
  charCache.clear();
  textMetricsEpoch.set(textMetricsEpoch.get() + 1);
}

/** Replaces the measurer (tests); `null` restores the canvas measurer. */
export function setTextMeasurer(m: TextMeasurer | null): void {
  measurerOverride = m;
  invalidateTextMetrics();
}

/**
 * The advance of ONE character in px, rounded to a whole pixel like the GDI glyph boxes of the original (the line
 * breaker sums per-character widths, `CEditString::Align`, so there is no kerning). Cached per font and character.
 */
export function charWidth(font: FontSpec, ch: string): number {
  const css = cssFont(font);
  const key = `${css}|${ch}`;
  const hit = charCache.get(key);
  if (hit !== undefined) return hit;
  const measure = measurerOverride ?? canvasMeasurer() ?? approximateMeasurer;
  const w = Math.max(0, Math.round(measure(css, ch)));
  charCache.set(key, w);
  return w;
}

/** The width of a string: the sum of its characters' widths (no kerning, `font-kerning: none`), in px. */
export function textWidth(font: FontSpec, text: string): number {
  let w = 0;
  for (const ch of text) w += charWidth(font, ch);
  return w;
}

/**
 * `document.fonts` loaded a face: the cached widths belong to the previous face, so they are dropped and the views
 * re-measure (`installFontWatch` is called once by the dev surfaces and the boot; it is a no-op without `document`).
 */
export function installFontWatch(doc?: { readonly fonts?: FontFaceSet }): () => void {
  const target: { readonly fonts?: FontFaceSet } | undefined =
    doc ?? (typeof document === 'undefined' ? undefined : document);
  const fonts = target?.fonts;
  if (typeof fonts?.addEventListener !== 'function') return () => undefined;
  const onDone = (): void => {
    invalidateTextMetrics();
  };
  fonts.addEventListener('loadingdone', onDone);
  return () => {
    fonts.removeEventListener('loadingdone', onDone);
  };
}
