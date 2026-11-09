// The title bar of a captioned frame (`CWndTitleBar`, WndNeuz.cpp:39-128; Plan/42 §5.11): up to seven buttons at a
// 16 px pitch in the order PIN, VIEW, EXTENSION, HELP, MINIMIZE, MAXIMIZE, CLOSE, one per set frame-style bit
// (CLOSE always, unless the frame sets `noCloseButton`). The bar is a `WBS_DOCKING` child (positioned relative to
// the frame's WINDOW rect): rect `(0, 4, n*16, 20)` moved to `x = frameWidth - n*16 - 7, y = 4` by `replace()`.

import { type Rect, rectXYWH } from '../layout/geometry';
import {
  WBS_EXTENSION,
  WBS_HELP,
  WBS_MAXIMIZEBOX,
  WBS_MINIMIZEBOX,
  WBS_PIN,
  WBS_VIEW,
  WTBID_CLOSE,
  WTBID_EXTENSION,
  WTBID_HELP,
  WTBID_MAX,
  WTBID_MIN,
  WTBID_PIN,
  WTBID_REPORT,
  WTYPE_BUTTON,
  Wnm,
} from '../layout/styles';
import { WndBase } from './WndBase';
import { Control, type ControlViewState } from './Control';
import { Observable } from './observable';

/** Pitch of the title-bar buttons. */
export const TITLE_BUTTON_PITCH = 16;
/** Sprite size of the textured buttons: `ButtWnd*.tga` is 60 x 14 = four 15 x 14 frames. */
export const TITLE_SPRITE = { w: 15, h: 14, frames: 4 } as const;

/** What a title-bar button looks like. */
export interface TitleButtonSpec {
  /** `WTBID_*`. */
  id: number;
  /** The frame-style bit that adds the button (`0` = always, CLOSE). */
  styleBit: number;
  /** Caption of the untextured buttons (`P`, `V`, `E`). */
  caption: string;
  /** The sprite (`Theme/<file>`), `null` for the untextured buttons. */
  sprite: string | null;
  /** A check/push-like toggle (the extension button). */
  toggle: boolean;
}

/** The buttons in creation order (ref/22 §4.2 table). */
export const TITLE_BUTTONS: readonly TitleButtonSpec[] = [
  { id: WTBID_PIN, styleBit: WBS_PIN, caption: 'P', sprite: null, toggle: false },
  { id: WTBID_REPORT, styleBit: WBS_VIEW, caption: 'V', sprite: null, toggle: false },
  { id: WTBID_EXTENSION, styleBit: WBS_EXTENSION, caption: 'E', sprite: null, toggle: true },
  { id: WTBID_HELP, styleBit: WBS_HELP, caption: '?', sprite: 'ButtWndHelp.tga', toggle: false },
  {
    id: WTBID_MIN,
    styleBit: WBS_MINIMIZEBOX,
    caption: '_',
    sprite: 'ButtWndMin.tga',
    toggle: false,
  },
  {
    id: WTBID_MAX,
    styleBit: WBS_MAXIMIZEBOX,
    caption: '|',
    sprite: 'ButtWndMax.tga',
    toggle: false,
  },
  { id: WTBID_CLOSE, styleBit: 0, caption: 'x', sprite: 'ButtWndExit.tga', toggle: false },
];

/** The buttons a frame style gets (`noClose` removes CLOSE; a frame without `WBS_CAPTION` has no bar at all). */
export function titleButtonsFor(frameStyle: number, noClose = false): TitleButtonSpec[] {
  return TITLE_BUTTONS.filter((b) =>
    b.id === WTBID_CLOSE ? !noClose : (frameStyle & b.styleBit) !== 0,
  );
}

export interface TitleButtonState extends ControlViewState {
  hover: boolean;
  pushed: boolean;
  /** `WBS_CHECK|WBS_PUSHLIKE` state of the extension button. */
  checked: boolean;
}

/** One button of the title bar. A press plays the click sound; a release inside notifies the frame. */
export class TitleButton extends Control<TitleButtonState> {
  readonly spec: TitleButtonSpec;
  /** Position inside the bar (button `n` sits at `x = n * 16`). */
  readonly index: number;
  #down = false;

  constructor(spec: TitleButtonSpec, index: number) {
    super(WTYPE_BUTTON, null, { hover: false, pushed: false, checked: false });
    this.spec = spec;
    this.index = index;
  }

  override onMouseEnter(): void {
    this.state.patch({ hover: true });
  }
  override onMouseLeave(): void {
    this.#down = false;
    this.state.patch({ hover: false, pushed: false });
  }
  override onLButtonDown(): void {
    this.#down = true;
    this.state.patch({ pushed: true });
    this.env.host.sound?.click();
  }
  override onLButtonUp(): void {
    const was = this.#down;
    this.#down = false;
    this.state.patch({ pushed: false });
    if (!was || !this.isWindowEnabled()) return;
    if (this.spec.toggle) this.state.patch({ checked: !this.state.get().checked });
    // CWndTitleBar::OnChildNotify forwards every notification to the frame
    this.parent?.onChildNotify(Wnm.CLICKED, this.spec.id, this);
  }
}

/** The title bar window: a docking child of the frame that owns the buttons. */
export class TitleBar extends WndBase {
  /** The bar rect in frame-window coordinates (an observable for the view). */
  readonly rectState = new Observable<Rect>(rectXYWH(0, 4, 0, 16), rectEq);
  /** `SetVisible` of the bar (`m_wndTitleBar.SetVisible(FALSE)`: the revival and countdown windows hide their buttons). */
  readonly visibleState = new Observable<boolean>(true);
  readonly buttons: TitleButton[] = [];

  /** `m_nButtonMax`. */
  get buttonCount(): number {
    return this.buttons.length;
  }

  override onInitialUpdate(): void {
    const frame = this.parent;
    if (!frame) return;
    const noClose = (frame as { noCloseButton?: boolean }).noCloseButton === true;
    const specs = titleButtonsFor(frame.style, noClose);
    specs.forEach((spec, n) => {
      const b = new TitleButton(spec, n);
      b.create(
        0x20000 /* WBS_CHILD */ | 0x200000 /* WBS_NOFRAME */,
        { l: n * TITLE_BUTTON_PITCH, t: 1, r: n * TITLE_BUTTON_PITCH + 16, b: 20 },
        this,
        spec.id,
      );
      // `FitTextureSize`: a textured button takes the size of one sprite frame (15 x 14)
      if (spec.sprite) {
        b.setWndRect({
          l: n * TITLE_BUTTON_PITCH,
          t: 1,
          r: n * TITLE_BUTTON_PITCH + TITLE_SPRITE.w,
          b: 1 + TITLE_SPRITE.h,
        });
      }
      this.buttons.push(b);
    });
    this.setWndRect({ l: 0, t: 4, r: specs.length * TITLE_BUTTON_PITCH, b: 20 });
  }

  /** `CWndTitleBar::Replace`: right-aligns the bar in the frame, 7 px from the edge, 4 px from the top. */
  replace(): void {
    const frame = this.parent;
    if (!frame) return;
    const w = frame.rectWindow.r - frame.rectWindow.l;
    this.move({ x: w - this.buttonCount * TITLE_BUTTON_PITCH - 7, y: 4 });
  }

  protected override onRectChanged(): void {
    this.rectState.set({ ...this.rectWindowValue });
  }

  protected override onVisibleChanged(): void {
    this.visibleState.set(this.visibleValue);
  }

  /** A button was clicked: the frame decides (`CWndTitleBar::OnChildNotify`). */
  override onChildNotify(msg: number, id: number, src: WndBase | null | object): boolean {
    return this.parent?.onChildNotify(msg, id, src as WndBase | null) ?? false;
  }
}

function rectEq(a: Rect, b: Rect): boolean {
  return a.l === b.l && a.t === b.t && a.r === b.r && a.b === b.b;
}
