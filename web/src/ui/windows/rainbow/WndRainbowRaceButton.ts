// `WndRainbowRaceButton` (`CWndRainbowRaceMiniGameButton`, `src:_Interface/WndRainbowRace.cpp:681-887`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-60): `APP_RAINBOWRACE_BUTTON` (803, 48 x 48), the button that opens the progress
// window while the race runs. No caption, no frame, `WBS_NOFOCUS` (Esc skips it: `ESC_SKIP_APPS`), at `x = root.right - 53`, `y = navigator.bottom + 5` while the navigator is open, else `root.top + 168`. It draws
// sprite 0 of `theme/texmaprainbowbutton.inc` (40 x 40): idle = the PULSING alpha (255 down to 100 and back in steps of 5 per 60 Hz frame, starting at 255 going down), the pointer over it = sprite 0 opaque, pushed with
// the pointer inside = sprite 2, pushed with the pointer outside = nothing (KEEP: `nTexNum = -1`). A press and a release inside the window WITHOUT a window move between them toggle the progress window
// (`request.onClick()`; FIX: the original needed the identical screen pixel, a drag moves the window).

import { APP_NAVIGATOR, APP_RAINBOWRACE_BUTTON } from '../../../gen/defs/resdata';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import { type Point, rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_CAPTION, WBS_NODRAWFRAME, WBS_NOFOCUS } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import { WndNeuz } from '../../wnd/WndNeuz';
import { RAINBOW_BUTTON_ATLAS } from './constants';
import { FrameClock } from './frameClock';
import { buttonPosition } from './geometry';
import type { ButtonRequest } from './ports';

const ID_SPRITE = 0x5300;

/** The alpha of the idle pulse: `m_nAlpha`, `m_bReverse` of `Process` (one step per 60 Hz frame). */
export class AlphaPulse {
  alpha = 255;
  #reverse = true;

  step(): number {
    this.alpha += this.#reverse ? -5 : 5;
    if (this.alpha < 100) {
      this.#reverse = false;
      this.alpha = 100;
    } else if (this.alpha > 255) {
      this.#reverse = true;
      this.alpha = 255;
    }
    return this.alpha;
  }
}

/** What the window draws: the sprite of the atlas (0 bright, 2 pushed) or nothing, and its alpha. */
export interface RaceButtonPaint {
  readonly sprite: 0 | 2 | null;
  readonly alpha: number;
}

/** `OnDraw`: idle -> sprite 0 with the pulse; hover -> sprite 0; hover and pushed -> sprite 2; pushed outside -> nothing. */
export function raceButtonPaint(
  hover: boolean,
  pushed: boolean,
  pulseAlpha: number,
): RaceButtonPaint {
  if (!hover && !pushed) return { sprite: 0, alpha: pulseAlpha };
  if (hover && !pushed) return { sprite: 0, alpha: 255 };
  if (hover && pushed) return { sprite: 2, alpha: 255 };
  return { sprite: null, alpha: 255 };
}

export interface RaceButtonDeps {
  readonly atlases: UiAtlases;
}

export class WndRainbowRaceButton extends WndNeuz {
  readonly appId = APP_RAINBOWRACE_BUTTON;
  override readonly processEveryFrame = true;
  readonly #request: ButtonRequest;
  readonly #deps: RaceButtonDeps;
  readonly #clock = new FrameClock();
  readonly #pulse = new AlphaPulse();
  #hover = false;
  #pushed = false;
  /** The window position at the press: a drag changes it. */
  #downAt: Point | null = null;
  #layer: SpriteLayer | null = null;
  #paint: RaceButtonPaint = { sprite: 0, alpha: 255 };

  constructor(request: ButtonRequest, deps: RaceButtonDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `WBS_NOFOCUS`: the button never takes the keyboard focus. */
  override initialize(parent: Parameters<WndNeuz['initialize']>[0], extraStyle = 0): boolean {
    return super.initialize(parent, extraStyle | WBS_NOFOCUS);
  }

  /** What the last frame drew. */
  get paint(): RaceButtonPaint {
    return this.#paint;
  }
  get hover(): boolean {
    return this.#hover;
  }
  get pushed(): boolean {
    return this.#pushed;
  }
  get layer(): SpriteLayer | null {
    return this.#layer;
  }

  override onInitialUpdate(): void {
    // `DelWndStyle( WBS_CAPTION ); m_wndTitleBar.SetVisible( FALSE )` and an empty `PaintFrame`: no title bar, no skin
    this.delWndStyle(WBS_CAPTION);
    this.addWndStyle(WBS_NODRAWFRAME);
    super.onInitialUpdate();
    const nav = this.env.wm.get(APP_NAVIGATOR);
    this.move(
      buttonPosition(
        this.env.wm.layoutRect,
        { x: rectWidth(this.rectWindow), y: rectHeight(this.rectWindow) },
        nav ? nav.rectWindow.b : null,
      ),
    );
    this.#layer = this.addControl(new SpriteLayer(), ID_SPRITE, this.getClientRect(false));
    this.#update();
  }

  #update(): void {
    this.#paint = raceButtonPaint(this.#hover, this.#pushed, this.#pulse.alpha);
    const dto = this.#deps.atlases.info(RAINBOW_BUTTON_ATLAS);
    const p = this.#paint;
    const cut = p.sprite === null ? undefined : dto?.sprites[p.sprite];
    const entries: SpriteEntry[] = [];
    if (dto && cut) {
      entries.push({
        key: 'button',
        path: dto.texture,
        x: 0,
        y: 0,
        w: cut.w,
        h: cut.h,
        src: { x: cut.x, y: cut.y },
        alpha: p.alpha / 255,
      });
    }
    this.#layer?.setEntries(entries);
  }

  /** Every frame the pulse advances (60 Hz steps from the elapsed time). */
  override process(now: number): void {
    super.process(now);
    const frames = this.#clock.advance(now);
    for (let i = 0; i < frames; i += 1) this.#pulse.step();
    this.#update();
  }

  override onMouseEnter(): void {
    this.#hover = true;
    this.#update();
  }
  override onMouseLeave(): void {
    this.#hover = false;
    this.#update();
  }

  override onLButtonDown(): void {
    this.#pushed = true;
    this.#downAt = { x: this.rectWindow.l, y: this.rectWindow.t };
    this.#update();
  }

  /** A second press within the double-click time is a press again: rapid clicks all count (the original compared the press and release points, which a double click keeps equal). */
  override onLButtonDblClk(): void {
    this.onLButtonDown();
  }

  override onLButtonUp(pt: Point): void {
    const inside =
      pt.x >= 0 &&
      pt.y >= 0 &&
      pt.x < rectWidth(this.rectClient) &&
      pt.y < rectHeight(this.rectClient);
    const moved =
      this.#downAt === null ||
      this.#downAt.x !== this.rectWindow.l ||
      this.#downAt.y !== this.rectWindow.t;
    const clicked = this.#pushed && inside && !moved;
    this.#pushed = false;
    this.#downAt = null;
    this.#update();
    if (clicked) this.#request.onClick();
  }

  /** The release outside the window: the press is over. */
  override onPointerRelease(): void {
    if (!this.#pushed) return;
    this.#pushed = false;
    this.#downAt = null;
    this.#update();
  }
}
