// `WndQuizEventButton` (`CWndQuizEventButton`, `src:_Interface/WndQuizEvent.cpp:264-456`, Plan/42 §19.4 P, 45 §28.8; WP-UI-59): `APP_QUIZ_BUTTON` (48 x 48), the button that opens the question window while a question is asked.
// No caption, no frame, `WBS_NOFOCUS`, placed at `( layout.l + 10, layout.t + height + 100 )` (= `t + 148`). It draws the sprites of `texMapQuizEventButton.inc` (40 x 40, from `QuizButton01.bmp`) on a canvas:
//   * the QUESTION state (`request.active()`): idle = sprite 0 with the PULSING alpha (255 -> 100 -> 255, FIX: a function of the elapsed time, not stepped per drawn frame), the pointer over it = sprite 0,
//     pushed with the pointer inside = sprite 2, pushed with the pointer outside = nothing (KEEP: `nTexNum = -1`);
//   * any other state: sprite 2;
//   * a press and a release inside the window = `request.onClick()` (FIX: the original needed the identical pixel).

import { APP_QUIZ_BUTTON } from '../../../gen/defs/resdata';
import { type Point, rectHeight, rectWidth } from '../../layout/geometry';
import type { WidcKey } from '../../layout/registry';
import { WBS_CAPTION, WBS_NODRAWFRAME, WBS_NOFOCUS } from '../../layout/styles';
import type { AtlasSpriteRef } from '../../skin/atlas';
import { WndNeuz } from '../../wnd/WndNeuz';
import { QUIZ_BUTTON_ATLAS, type QuizButtonPaint, quizButtonPaint } from './paint';
import type { QuizButtonRequest } from './ports';

/** The canvas of the sprite. */
const CANVAS = 'quizButton';

export interface QuizButtonDeps {
  readonly sprite: (atlas: string, index: number) => Promise<AtlasSpriteRef | undefined>;
}

export class WndQuizEventButton extends WndNeuz {
  readonly appId = APP_QUIZ_BUTTON;
  override readonly processEveryFrame = true;
  readonly #request: QuizButtonRequest;
  readonly #deps: QuizButtonDeps;
  #hover = false;
  #pushed = false;
  /** The clock of the first process: the pulse counts from there. */
  #origin = Number.NaN;
  #now = 0;
  #paint: QuizButtonPaint = { sprite: 0, alpha: 255 };
  readonly #sprites = new Map<number, AtlasSpriteRef | null>();
  readonly #bitmaps = new Map<number, HTMLCanvasElement>();

  constructor(request: QuizButtonRequest, deps: QuizButtonDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `WBS_NOFOCUS`: the button never takes the keyboard focus. */
  override initialize(parent: Parameters<WndNeuz['initialize']>[0], extraStyle = 0): boolean {
    return super.initialize(parent, extraStyle | WBS_NOFOCUS);
  }

  /** What the last frame drew (sprite index or `null`, alpha 0..255). */
  get paint(): QuizButtonPaint {
    return this.#paint;
  }
  get hover(): boolean {
    return this.#hover;
  }
  get pushed(): boolean {
    return this.#pushed;
  }

  override onInitialUpdate(): void {
    // `DelWndStyle( WBS_CAPTION ); m_wndTitleBar.SetVisible( FALSE )` and an empty `PaintFrame`: no title bar, no skin
    this.delWndStyle(WBS_CAPTION);
    this.addWndStyle(WBS_NODRAWFRAME);
    super.onInitialUpdate();
    // `( rectRoot.left + 10, rectRoot.top + rectWindow.Height() + 100 )`
    const root = this.env.wm.layoutRect;
    this.move({ x: root.l + 10, y: root.t + rectHeight(this.rectWindowValue) + 100 });
    this.addCustomCanvas(
      CANVAS,
      { l: 0, t: 0, r: rectWidth(this.rectWindowValue), b: rectHeight(this.rectWindowValue) },
      true,
    );
    void this.#load();
    this.#update(this.env.host.now());
  }

  async #load(): Promise<void> {
    for (const i of [0, 2]) {
      const s = await this.#deps.sprite(QUIZ_BUTTON_ATLAS, i).catch(() => undefined);
      if (this.destroyed) return;
      this.#sprites.set(i, s ?? null);
    }
    this.invalidate();
  }

  // ---- the state ---------------------------------------------------------------------------------------------------------------

  #update(now: number): void {
    if (Number.isNaN(this.#origin)) this.#origin = now;
    this.#now = now;
    const next = quizButtonPaint(
      this.#request.active(),
      this.#hover,
      this.#pushed,
      now - this.#origin,
    );
    if (next.sprite !== this.#paint.sprite || next.alpha !== this.#paint.alpha) {
      this.#paint = next;
      this.invalidate();
    }
  }

  /** Every frame: the pulse advances. */
  override process(now: number): void {
    super.process(now);
    this.#update(now);
  }

  /** The paint as of the clock `ms` after the first process (tests): refreshes the state at that time. */
  paintAt(ms: number): QuizButtonPaint {
    if (Number.isNaN(this.#origin)) this.#origin = this.#now;
    this.#update(this.#origin + ms);
    return this.#paint;
  }

  override onMouseEnter(): void {
    this.#hover = true;
    this.#update(this.#now);
  }
  override onMouseLeave(): void {
    this.#hover = false;
    this.#update(this.#now);
  }

  override onLButtonDown(): void {
    this.#pushed = true;
    this.#update(this.#now);
  }

  override onLButtonUp(pt: Point): void {
    const inside =
      pt.x >= 0 &&
      pt.y >= 0 &&
      pt.x < rectWidth(this.rectClient) &&
      pt.y < rectHeight(this.rectClient);
    const clicked = this.#pushed && inside;
    this.#pushed = false;
    this.#update(this.#now);
    if (clicked) this.#request.onClick();
  }

  /** The release outside the window: the press is over. */
  override onPointerRelease(): void {
    if (!this.#pushed) return;
    this.#pushed = false;
    this.#update(this.#now);
  }

  // ---- the pixels ------------------------------------------------------------------------------------------------------------------

  #bitmap(index: number): HTMLCanvasElement | null {
    const known = this.#bitmaps.get(index);
    if (known) return known;
    const s = this.#sprites.get(index);
    if (!s || typeof document === 'undefined') return null;
    const c = document.createElement('canvas');
    c.width = s.w;
    c.height = s.h;
    const ctx = c.getContext('2d');
    if (!ctx) return null;
    const px = new Uint8ClampedArray(s.w * s.h * 4);
    for (let y = 0; y < s.h; y += 1) {
      const from = ((s.sy + y) * s.img.width + s.sx) * 4;
      px.set(s.img.data.subarray(from, from + s.w * 4), y * s.w * 4);
    }
    ctx.putImageData(new ImageData(px, s.w, s.h), 0, 0);
    this.#bitmaps.set(index, c);
    return c;
  }

  override onDraw(_id: WidcKey, ctx: CanvasRenderingContext2D): void {
    const p = this.#paint;
    if (p.sprite === null) return;
    const bmp = this.#bitmap(p.sprite);
    if (!bmp) return;
    ctx.globalAlpha = p.alpha / 255;
    ctx.drawImage(bmp, 0, 0);
    ctx.globalAlpha = 1;
  }
}
