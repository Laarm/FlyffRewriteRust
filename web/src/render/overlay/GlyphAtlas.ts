// Overlay fonts and text drawing (Plan/41 §19.2; ITheme.cpp:91-121, d3dfont.cpp:95-360, `propLang.txt` row ENG).
//
// The original rasterises Verdana glyphs into texture pages and draws every string as textured quads. The overlay of
// this port draws with Canvas2D (`fillText`): the glyph rasteriser of the browser is the atlas. What stays exact is
// everything the picture depends on: the two font slots, the text width (the SUM of the per-character advances, no
// kerning: `GetTextExtent`), the fixed Verdana line cells, and the 2 px outline made of eight copies of the string in
// a fixed order and colour, drawn BEFORE the text (`CD3DFont::DrawText`, d3dfont.cpp:333-356).

/** The two slots of the overlay (`FontWorld` and `m_pFontText`). */
export type OverlayFontKey = 'world' | 'text';

export interface OverlayFont {
  readonly key: OverlayFontKey;
  /** `MulDiv(pt, 96, 72)`: 9 pt = 12 px. */
  readonly px: number;
  /** CSS weight (500 = `FW_MEDIUM`). */
  readonly weight: number;
  /** Outline distance in px, 0 for none. */
  readonly outline: number;
  /** Verdana's GDI cell height at this size (`GetMaxHeight` without the outline). */
  readonly cell: number;
}

export const OVERLAY_FONTS: Readonly<Record<OverlayFontKey, OverlayFont>> = {
  world: { key: 'world', px: 12, weight: 500, outline: 2, cell: 15 },
  text: { key: 'text', px: 12, weight: 500, outline: 0, cell: 15 },
};

/** The line height of a font slot (`GetMaxHeight`; an outlined font adds `2 * outline`). */
export function lineHeightOf(font: OverlayFont): number {
  return font.cell + 2 * font.outline;
}

/**
 * The eight outline copies in the order of `d3dfont.cpp:342-357` (`m_nOutLine` = 2): (+,+), (-,-), (-,+), (+,-),
 * (+,0), (-,0), (0,-), (0,+); the text follows at (0, 0).
 */
export function outlineOffsets(outline: number): readonly (readonly [number, number])[] {
  const o = outline;
  return [
    [o, o],
    [-o, -o],
    [-o, o],
    [o, -o],
    [o, 0],
    [-o, 0],
    [0, -o],
    [0, o],
  ];
}

/** `m_dwBgColor` 0x3C3C3C quantised to 4 bits per channel and read as `n / 16`: 3 / 16 = 0.1875. */
export const OUTLINE_CHANNEL = 3 / 16;

/** The outline colour as 8-bit channels. */
export const OUTLINE_RGB: readonly [number, number, number] = [
  Math.round(OUTLINE_CHANNEL * 255),
  Math.round(OUTLINE_CHANNEL * 255),
  Math.round(OUTLINE_CHANNEL * 255),
];

/** The bundled fallback face (declared once in `styles/fonts.css`; the same name the UI uses). */
export const FALLBACK_FAMILY = 'Flyff DejaVu Sans';

let fallbackFirst = false;

/** `font=fallback` (deterministic screenshots): the bundled face first in every stack. Invalidates measurements. */
export function setOverlayFallbackFirst(on: boolean): void {
  fallbackFirst = on;
}

/** `font-family` of the overlay text: Verdana first, the bundled face second. */
export function overlayFamily(): string {
  return fallbackFirst
    ? `"${FALLBACK_FAMILY}", Verdana, sans-serif`
    : `Verdana, "${FALLBACK_FAMILY}", sans-serif`;
}

/** The `ctx.font` string of a slot. */
export function overlayCssFont(font: OverlayFont): string {
  return `${String(font.weight)} ${String(font.px)}px ${overlayFamily()}`;
}

/** `0xAARRGGBB` -> `rgba(r,g,b,a)`; `alpha` (0..255) replaces the alpha byte when given. */
export function argbCss(argb: number, alpha?: number): string {
  const c = argb >>> 0;
  const a = alpha ?? c >>> 24;
  return `rgba(${String((c >>> 16) & 255)},${String((c >>> 8) & 255)},${String(c & 255)},${(a / 255).toFixed(3)})`;
}

// ---- measurement ---------------------------------------------------------------------------------------------

/** Text width in px (`GetTextExtent(...).cx`). */
export interface TextMeter {
  width(font: OverlayFont, text: string): number;
}

/** The advance of one character in a CSS font (`ctx.measureText`). */
export type AdvanceFn = (cssFont: string, ch: string) => number;

/** The canvas advance function, or `null` where there is no canvas (Node tests, happy-dom). */
export function canvasAdvance(): AdvanceFn | null {
  let ctx: { font: string; measureText(t: string): { width: number } } | null = null;
  try {
    if (typeof OffscreenCanvas !== 'undefined') {
      ctx = new OffscreenCanvas(1, 1).getContext('2d');
    } else if (typeof document !== 'undefined') {
      ctx = document.createElement('canvas').getContext('2d');
    }
  } catch {
    ctx = null;
  }
  if (!ctx) return null;
  const c = ctx;
  return (cssFont, ch) => {
    c.font = cssFont;
    return c.measureText(ch).width;
  };
}

/**
 * The text meter: the width of a string is the sum of the advances of its characters (no kerning, so it equals the
 * original's `GetTextExtent` for the same face), each measured once per font and cached.
 */
export class GlyphMeter implements TextMeter {
  readonly #advance: AdvanceFn;
  readonly #cache = new Map<string, number>();

  constructor(advance: AdvanceFn = canvasAdvance() ?? ((_f, ch) => (ch === ' ' ? 4 : 7))) {
    this.#advance = advance;
  }

  /** The advance of one character (rounded like GDI's integer advances). */
  advanceOf(font: OverlayFont, ch: string): number {
    const css = overlayCssFont(font);
    const key = `${css}|${ch}`;
    let w = this.#cache.get(key);
    if (w === undefined) {
      w = Math.round(this.#advance(css, ch));
      this.#cache.set(key, w);
    }
    return w;
  }

  width(font: OverlayFont, text: string): number {
    let w = 0;
    for (const ch of text) w += this.advanceOf(font, ch);
    return w;
  }

  /** A font finished loading: the measurements are stale. */
  invalidate(): void {
    this.#cache.clear();
  }
}

// ---- drawing -------------------------------------------------------------------------------------------------

/** The part of `CanvasRenderingContext2D` the overlay uses (a recording fake in the tests). */
export interface Ctx2D {
  font: string;
  fillStyle: string | CanvasGradient | CanvasPattern;
  globalAlpha: number;
  textBaseline: CanvasTextBaseline;
  imageSmoothingEnabled: boolean;
  fillText(text: string, x: number, y: number): void;
  fillRect(x: number, y: number, w: number, h: number): void;
  clearRect(x: number, y: number, w: number, h: number): void;
  drawImage(
    image: CanvasImageSource,
    sx: number,
    sy: number,
    sw: number,
    sh: number,
    dx: number,
    dy: number,
    dw: number,
    dh: number,
  ): void;
  save(): void;
  restore(): void;
  /** Multiplies the current matrix (`CanvasRenderingContext2D.transform`); unlike `setTransform` it keeps the device-pixel-ratio scale. */
  transform(a: number, b: number, c: number, d: number, e: number, f: number): void;
}

/**
 * Draws `text` with its top-left at `(x, y)`: the eight outline copies (colour of {@link OUTLINE_RGB} with the alpha of
 * the text) when the slot has an outline, then the text.
 */
export function drawOutlinedText(
  ctx: Ctx2D,
  font: OverlayFont,
  text: string,
  x: number,
  y: number,
  argb: number,
): void {
  if (text === '') return;
  ctx.font = overlayCssFont(font);
  ctx.textBaseline = 'top';
  if (font.outline > 0) {
    const a = (argb >>> 24) & 255;
    ctx.fillStyle = `rgba(${String(OUTLINE_RGB[0])},${String(OUTLINE_RGB[1])},${String(OUTLINE_RGB[2])},${(a / 255).toFixed(3)})`;
    for (const [dx, dy] of outlineOffsets(font.outline)) ctx.fillText(text, x + dx, y + dy);
  }
  ctx.fillStyle = argbCss(argb);
  ctx.fillText(text, x, y);
}
