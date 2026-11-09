// Captions (Plan/45 §7.11; `CCaption`, WndWorld.cpp:93-225): big titles that fly in at the top of the world view. Per 60 Hz
// frame the active caption's `addScale += 0.002` and `scale += addScale` (at most 1.0); the next queued caption starts when
// the previous one passed 0.1; when the last one reaches 1.0 a 5 s timer starts, then every caption loses 3 alpha per
// frame; a caption added after the end clears the old set. Drawn centred with the scale factor `7 - 6 * scale` (7x at the
// start, 1x settled), colour `ARGB(scale * 255 - (255 - alpha), 250, 250, 255)`, the outlined caption font; the
// vertical origin is `184 / 768` of the view height, each caption `size.cy` below the previous one.

/** The settled hold before the fade (`SEC(5)`). */
export const CAPTION_HOLD_MS = 5000;

export interface Caption {
  readonly text: string;
  /** The small caption font (`CAPTION_SMALL`, region titles after the first line) instead of the big title font. */
  readonly small: boolean;
  /** The text size in the caption font at scale 1 (`GetTextExtent`). */
  readonly size: { readonly w: number; readonly h: number };
  scale: number;
  addScale: number;
  alpha: number;
}

export class Captions {
  readonly list: Caption[] = [];
  /** `m_nCount`: how many captions are visible / animated. */
  count = 0;
  ended = false;
  #timerEnd = 0;

  /** `AddCaption`: a caption added after the end (and after the hold) clears the old set. */
  add(text: string, small: boolean, size: { w: number; h: number }, nowMs: number): void {
    if (this.ended && nowMs >= this.#timerEnd) this.clear();
    this.list.push({ text, small, size, scale: 0, addScale: 0, alpha: 255 });
    this.ended = false;
  }

  clear(): void {
    this.list.length = 0;
    this.count = 0;
    this.ended = false;
  }

  /** `CCaption::Process` once (one 60 Hz frame). */
  frame(nowMs: number): void {
    if (this.count === 0 && this.list.length > 0) this.count = 1;
    if (this.ended) {
      if (nowMs >= this.#timerEnd) {
        for (const c of this.list) c.alpha = Math.max(0, c.alpha - 3);
      }
      return;
    }
    for (let i = 0; i < this.count; i += 1) {
      const c = this.list[i];
      if (!c) continue;
      c.addScale += 0.002;
      c.scale += c.addScale;
      if (c.scale > 1) c.scale = 1;
      if (c.scale > 0.1 && i === this.count - 1) {
        this.count += 1;
        if (this.count > this.list.length) {
          this.count = this.list.length;
          if (c.scale >= 1) {
            this.ended = true;
            this.#timerEnd = nowMs + CAPTION_HOLD_MS;
          }
        }
        break;
      }
    }
  }

  /** `(7 - 6 * scale)`: 7x at the start, 1x when settled. */
  static drawScale(c: Pick<Caption, 'scale'>): number {
    return 7 - c.scale * 6;
  }

  /** The text alpha byte of `D3DCOLOR_ARGB(scale * 255 - (255 - alpha), ...)` (truncated like the C++ `(int)` cast). */
  static drawAlpha(c: Pick<Caption, 'scale' | 'alpha'>): number {
    return Math.trunc(c.scale * 255) - (255 - c.alpha);
  }
}
