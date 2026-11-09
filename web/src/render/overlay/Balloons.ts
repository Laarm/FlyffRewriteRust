// Speech balloons, emoticons and vendor titles (Plan/41 §19.6; DialogMsg.cpp:161-1028): the registry of what floats
// above which mover, the text layout (wrap, sizes rounded up to 16) and the geometry of the 9-patch, its tail and the
// fade. The Canvas2D drawing of the produced items is `OverlayRenderer`'s.
//
// Requests arrive through `RenderFx.balloon / emoticon / vendorTitle` (doc 45's chat routing, §8.5). A new balloon (or
// emoticon) replaces the mover's previous one; the lifetime is 5 000 ms; the vendor title lasts until it is removed.

import { even } from './anchor';
import { OVERLAY_FONTS, lineHeightOf, type TextMeter } from './GlyphAtlas';
import type { DrawItem } from './types';

/** `m_timer.Set(5000)`. */
export const BALLOON_LIFETIME_MS = 5000;
/** The balloon texture alpha (`nAlpha`). */
export const BALLOON_ALPHA = 200;
/** The fade-in: while more than this remains the alpha is lower. */
export const BALLOON_FADE_MS = 4000;

export type BalloonKind = 0 | 1 | 2 | 3;

/** The atlas of the 9-patch by kind (`NORMAL`, `PARTY`, `GUILD`; a shout falls back to the normal one). */
export const BALLOON_ATLAS: Readonly<Record<BalloonKind, string>> = {
  0: 'theme/texdialogbox.inc',
  1: 'theme/texdialogboxparty.inc',
  2: 'theme/texdialogboxguild.inc',
  3: 'theme/texdialogbox.inc',
};

/** The three pieces of the vendor strip (`m_pTex[0..2]`). */
export const VENDOR_TEXTURES = [
  'theme/texdialogbox_left.tga',
  'theme/texdialogbox_center.tga',
  'theme/texdialogbox_right.tga',
] as const;

/** `roundUp16`: `(x / 16) * 16 + (x % 16 ? 16 : 0)` with integer division. */
export function roundUp16(x: number): number {
  const v = Math.trunc(x);
  return Math.trunc(v / 16) * 16 + (v % 16 !== 0 ? 16 : 0);
}

/** `#c` + 8 hex, `#b #u #s #n?` and the other markup codes removed (the balloon draws black text). */
export function stripMarkup(text: string): string {
  let out = '';
  const n = text.length;
  for (let i = 0; i < n; i += 1) {
    const ch = text.charAt(i);
    if (ch === '#') {
      i += 1;
      if (i >= n) break;
      const code = text.charAt(i);
      if (code === 'c') {
        i += 8; // the letter itself is the loop's increment: 8 colour characters follow
        continue;
      }
      if (code === 'l') {
        i += 4;
        continue;
      }
      if (code === 'n') {
        i += 1;
        continue;
      }
      if (code === 'u' || code === 'b' || code === 's') continue;
      out += `#${code}`;
    } else if (ch === '\\' && text.charAt(i + 1) === 'n') {
      out += '\n';
      i += 1;
    } else {
      out += ch;
    }
  }
  return out;
}

/** The characters after which a line may break (`CEditString::Align`). */
const BREAK_CHARS = " +-*^/%=;(),':{}.";

/**
 * The line breaker of `CEditString::Align` (Plan/42 §8.3): a break opportunity after any of `BREAK_CHARS`; a non-space
 * character that overflows starts the next line after the last opportunity (word alignment), or before itself when
 * the line has none; a space never breaks; `\n` ends a line. A character that does not fit alone gets its own line.
 */
export function wrapText(
  text: string,
  width: number,
  meter: TextMeter,
  font = OVERLAY_FONTS.text,
): string[] {
  const chars = Array.from(text).filter((c) => c !== '\r');
  const n = chars.length;
  const lines: string[] = [];
  let offset = 0;
  while (offset < n) {
    const start = offset;
    let w = 0;
    let nSpace = 0;
    let end: number;
    for (;;) {
      const ch = chars[offset] ?? '';
      if (BREAK_CHARS.includes(ch)) nSpace = offset + 1;
      if (ch === '\n') {
        end = offset;
        offset += 1;
        break;
      }
      w += meter.width(font, ch);
      if (ch !== ' ' && w > width) {
        if (nSpace > start) offset = nSpace;
        else if (offset === start) offset += 1;
        end = offset;
        break;
      }
      offset += 1;
      if (offset >= n) {
        end = n;
        break;
      }
    }
    lines.push(chars.slice(start, end).join(''));
  }
  return lines;
}

/** The size of a text balloon (`AddMessage`): `cx` 160 with a 144 px wrap from an extent of 160, else rounded up. */
export interface BalloonSize {
  cx: number;
  cy: number;
  lines: string[];
}

export function balloonSize(text: string, meter: TextMeter): BalloonSize {
  const font = OVERLAY_FONTS.text;
  const plain = stripMarkup(text);
  const extent = meter.width(font, plain);
  const cx = extent >= 160 ? 160 : roundUp16(extent + 16);
  const lines = wrapText(plain, cx - 16, meter, font);
  const cy = roundUp16(lines.length * lineHeightOf(font) + 16);
  return { cx, cy, lines };
}

/** The opacity of a balloon with `leftMs` remaining: 0 at the start, 200 from a second in (`nAlpha`). */
export function balloonAlpha(leftMs: number): number {
  if (leftMs > BALLOON_FADE_MS)
    return Math.trunc(200 - Math.trunc(((leftMs - BALLOON_FADE_MS) * 200) / 1000));
  return BALLOON_ALPHA;
}

/** `texDialogBoxShout0N.bmp` by the text length (`_tcsclen`). */
export function shoutTexture(length: number): string {
  let n: number;
  if (length <= 1) n = 0;
  else if (length <= 3) n = 1;
  else if (length <= 5) n = 2;
  else if (length <= 9) n = 3;
  else if (length <= 20) n = 4;
  else if (length <= 40) n = 5;
  else if (length <= 60) n = 6;
  else n = 7;
  return `theme/texdialogboxshout0${String(n)}.bmp`;
}

/** The pixel geometry of a balloon box: the nine pieces and the tail, relative to its top-left corner. */
export interface BalloonGeometry {
  /** `[x, y, w, h]` of the nine patches in row-major order (sprite index = position). */
  patches: readonly (readonly [number, number, number, number])[];
  /** The tail's top-left (a 8 x 8 sprite, index 9), `null` for boxes that are too narrow. */
  tail: readonly [number, number] | null;
  /** `cx / 8`. */
  cells: number;
  /** The width of the middle column (`0.7 cx` up to five cells, else `0.9 cx`). */
  middleW: number;
  /** The height of the middle row (`0.75 cy`). */
  middleH: number;
}

export function balloonGeometry(cx: number, cy: number): BalloonGeometry {
  const edge = 8;
  const cells = Math.trunc(cx / 8);
  const middleW = cells <= 5 ? cx * 0.7 : cx * 0.9;
  const middleH = cy * 0.75;
  const cols = [edge, middleW, edge];
  const rows = [edge, middleH, edge];
  const patches: [number, number, number, number][] = [];
  let y = 0;
  for (const h of rows) {
    let x = 0;
    for (const w of cols) {
      patches.push([x, y, w, h]);
      x += w;
    }
    y += h;
  }
  let tail: [number, number] | null = null;
  if (cells >= 6) tail = [Math.trunc((3 * cells) / 5) * 8, y];
  else if (cells === 4 || cells === 5) tail = [3 * 8, y];
  else if (cells === 3) tail = [1 * 8, y];
  return { patches, tail, cells, middleW, middleH };
}

// ---- the registry --------------------------------------------------------------------------------------------

interface TextBalloon {
  readonly type: 'text';
  readonly kind: BalloonKind;
  readonly plainLength: number;
  readonly size: BalloonSize;
  readonly untilMs: number;
}

interface EmoticonBalloon {
  readonly type: 'emoticon';
  readonly set: 'chat' | 'dialog';
  readonly index: number;
  readonly untilMs: number;
}

type Entry = TextBalloon | EmoticonBalloon;

interface VendorTitle {
  readonly text: string;
  readonly size: { cx: number; cy: number };
  readonly color: number;
}

/** The chat and dialog emoticon atlases. */
export const EMOTICON_ATLAS: Readonly<Record<'chat' | 'dialog', string>> = {
  chat: 'theme/texchatemoticon.inc',
  dialog: 'theme/texdialogemoticon.inc',
};

export class BalloonLayer {
  readonly #entries = new Map<number, Entry>();
  readonly #vendors = new Map<number, VendorTitle>();
  readonly #meter: TextMeter;
  readonly #rng: () => number;

  /** @param rng picks the colour of a vendor title (red or blue, `rand() % 2` in the original) */
  constructor(meter: TextMeter, rng: () => number = Math.random) {
    this.#meter = meter;
    this.#rng = rng;
  }

  get size(): number {
    return this.#entries.size;
  }

  /** `AddMessage`: a text balloon (replaces the mover's previous balloon or emoticon). */
  add(id: number, text: string, kind: BalloonKind, nowMs: number): void {
    const size = balloonSize(text, this.#meter);
    this.#entries.set(id, {
      type: 'text',
      kind,
      plainLength: Array.from(stripMarkup(text)).length,
      size,
      untilMs: nowMs + BALLOON_LIFETIME_MS,
    });
  }

  /** `AddEmoticon` / `AddEmoticonUser`. */
  addEmoticon(id: number, set: 'chat' | 'dialog', index: number, nowMs: number): void {
    this.#entries.set(id, { type: 'emoticon', set, index, untilMs: nowMs + BALLOON_LIFETIME_MS });
  }

  /** `RemoveMessage` / `ClearMessage(obj)`. */
  remove(id: number): void {
    this.#entries.delete(id);
  }

  /** The mover shows a balloon (the quest emoticon stays hidden meanwhile). */
  has(id: number): boolean {
    return this.#entries.has(id);
  }

  /** `AddVendorMessage`: the title strip above an open shop (line breaks become spaces). */
  addVendor(id: number, title: string): void {
    const text = title.replace(/\n/g, ' ');
    const extent = this.#meter.width(OVERLAY_FONTS.text, text);
    const cx = roundUp16(extent + 16);
    const lines = wrapText(text, cx - 16, this.#meter);
    const cy = roundUp16(lines.length * lineHeightOf(OVERLAY_FONTS.text) + 16);
    this.#vendors.set(id, {
      text,
      size: { cx, cy },
      color: this.#rng() < 0.5 ? 0xffff0000 : 0xff0000ff,
    });
  }

  /** `RemoveVendorMessage`. */
  removeVendor(id: number): void {
    this.#vendors.delete(id);
  }

  hasVendor(id: number): boolean {
    return this.#vendors.has(id);
  }

  /** The mover was deleted. */
  removeFor(id: number): void {
    this.#entries.delete(id);
    this.#vendors.delete(id);
  }

  /** `ClearAllMessage`: the world changed. */
  clear(): void {
    this.#entries.clear();
    this.#vendors.clear();
  }

  /** Drops what timed out (a quest emoticon of the mover shows again); returns the ids. */
  expire(nowMs: number): number[] {
    const gone: number[] = [];
    for (const [id, e] of this.#entries) {
      if (nowMs >= e.untilMs) {
        this.#entries.delete(id);
        gone.push(id);
      }
    }
    return gone;
  }

  /** The ids that have a balloon, an emoticon or a vendor title. */
  ids(): number[] {
    return [...new Set([...this.#entries.keys(), ...this.#vendors.keys()])];
  }

  /**
   * The draw items of what floats above `id`. `anchor` is the head projection (CSS pixels, y down). `null` when there
   * is nothing for the mover. The vendor strip comes after the balloon.
   */
  layoutFor(id: number, anchor: { x: number; y: number }, nowMs: number): DrawItem[] | null {
    const out: DrawItem[] = [];
    const e = this.#entries.get(id);
    if (e) out.push(...this.#layoutEntry(e, anchor, nowMs));
    const v = this.#vendors.get(id);
    if (v) out.push(...this.#layoutVendor(v, anchor));
    return out.length === 0 ? null : out;
  }

  #layoutEntry(e: Entry, anchor: { x: number; y: number }, nowMs: number): DrawItem[] {
    if (e.type === 'emoticon') {
      return [
        {
          kind: 'sprite',
          atlas: EMOTICON_ATLAS[e.set],
          index: e.index,
          x: even(Math.trunc(anchor.x - 32 / 2)),
          y: Math.trunc(anchor.y - 32),
          alpha: 255,
        },
      ];
    }
    const left = Math.max(0, e.untilMs - nowMs);
    const alpha = balloonAlpha(left);
    const { cx, cy, lines } = e.size;
    const x = anchor.x - Math.trunc(cx / 2);
    // KEEP-QUIRK: the screen y is scaled by 0.9
    const y = (anchor.y - cy) * 0.9;
    const g = balloonGeometry(cx, cy);
    const out: DrawItem[] = [];
    let shout = false;
    if (e.kind === 3) {
      // a missing shout texture falls back to the 9-patch (the renderer checks the asset); both are produced
      shout = true;
      out.push({
        kind: 'image',
        path: shoutTexture(e.plainLength),
        x: Math.trunc(x - 20),
        y: Math.trunc(y - 20),
        alpha,
        scaleX: 1,
        scaleY: 1,
        dw: Math.trunc(g.middleW + 50),
        dh: Math.trunc(g.middleH + 40),
      });
    }
    if (!shout) {
      const atlas = BALLOON_ATLAS[e.kind];
      g.patches.forEach(([px, py, pw, ph], i) => {
        out.push({ kind: 'sprite', atlas, index: i, x: x + px, y: y + py, alpha, dw: pw, dh: ph });
      });
      if (g.tail) {
        out.push({
          kind: 'sprite',
          atlas,
          index: 9,
          x: Math.trunc(x + g.tail[0]),
          y: Math.trunc(y + g.tail[1]),
          alpha,
        });
      }
    }
    const font = OVERLAY_FONTS.text;
    const color = ((alpha << 24) | 0) >>> 0;
    lines.forEach((line, i) => {
      out.push({
        kind: 'text',
        font,
        text: line,
        x: Math.trunc(x + 8),
        y: Math.trunc(y + 8) + i * lineHeightOf(font),
        color,
      });
    });
    return out;
  }

  #layoutVendor(v: VendorTitle, anchor: { x: number; y: number }): DrawItem[] {
    const { cx, cy } = v.size;
    const x = anchor.x - Math.trunc(cx / 2);
    const y = anchor.y - cy;
    const gap = 42;
    const font = OVERLAY_FONTS.text;
    const out: DrawItem[] = [
      {
        kind: 'image',
        path: VENDOR_TEXTURES[0],
        x: Math.trunc(x - 24),
        y: Math.trunc(y - gap),
        alpha: 200,
        scaleX: 1,
        scaleY: 1,
      },
      {
        kind: 'image',
        path: VENDOR_TEXTURES[1],
        x: Math.trunc(x + 8),
        y: Math.trunc(y - gap),
        alpha: 200,
        scaleX: 1,
        scaleY: 1,
        dw: cx,
        dh: 32,
      },
      {
        kind: 'image',
        path: VENDOR_TEXTURES[2],
        x: Math.trunc(x + 8) + cx,
        y: Math.trunc(y - gap),
        alpha: 200,
        scaleX: 1,
        scaleY: 1,
      },
    ];
    const color = ((250 << 24) | (v.color & 0xffffff)) >>> 0;
    out.push({
      kind: 'text',
      font,
      text: v.text,
      x: Math.trunc(x + 8),
      y: Math.trunc(y + 8) - (gap - 2),
      color,
    });
    return out;
  }
}
