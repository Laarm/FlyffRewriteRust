// `WndChangeFace` (`CWndChangeFace`, `src:_Interface/WndChangeFace.cpp:21-130`, ref/22 §17 L, Plan/42 §19.4 L; WP-UI-35): `APP_CHANGEFACE`
// (464 x 272), "Please select a face. You can change your face 2 times at maximum.". The five faces are painted in the window's picture
// (`WndChangeFace.tga`, 86 px apart from x = 10, 170 px high from y = 5); a click on one selects it (`OnLButtonUp`:
// `(x - 10) / 86` in 0..4 with `y` in 0..179, the division truncating toward zero) and a blue frame, drawn twice (the rect and the rect inflated by one
// pixel), marks the chosen one. OK sends the face and closes, Cancel and the close button only close. No menu or item opens the window in the shipped
// client; the `appearance` feature opens it for the tools and the gallery.

import { APP_CHANGEFACE, WIDC_CANCEL, WIDC_OK } from '../../../gen/defs/resdata';
import { RectLayer, argbCss, type RectEntry } from '../../controls/RectLayer';
import type { Shortcut } from '../../dnd/shortcut';
import type { Point } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { FACE_HEIGHT, FACE_LEFT, FACE_PITCH, FACE_TOP, faceAt } from './geometry';
import type { ChangeFaceRequest } from './ports';

/** `RenderRect( rect, 0xff0000ff )`. */
const FRAME_COLOR = 0xff0000ff;

export { FACE_HEIGHT, FACE_LEFT, FACE_PITCH, FACE_TOP, faceAt };

export class WndChangeFace extends WndNeuz {
  readonly appId = APP_CHANGEFACE;
  readonly #request: ChangeFaceRequest;
  #selected = 0;
  #frame: RectLayer | null = null;

  constructor(request: ChangeFaceRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `m_nSelectFace`. */
  get selected(): number {
    return this.#selected;
  }
  get frameLayer(): RectLayer | null {
    return this.#frame;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#frame = this.addControl(new RectLayer(), 0x7e21, this.getClientRect(false));
    this.#paint();
    this.moveParentCenter();
  }

  #paint(): void {
    const x = FACE_LEFT + this.#selected * FACE_PITCH;
    const outline = { color: argbCss(FRAME_COLOR), width: 1 };
    const entries: RectEntry[] = [
      { key: 'frame', x, y: FACE_TOP, w: FACE_PITCH, h: FACE_HEIGHT, outline },
      {
        key: 'frame2',
        x: x - 1,
        y: FACE_TOP - 1,
        w: FACE_PITCH + 2,
        h: FACE_HEIGHT + 2,
        outline,
      },
    ];
    this.#frame?.setEntries(entries);
  }

  /** `OnLButtonUp` (the window is the client of the layout): the face under the pointer is selected. */
  override onLButtonUp(pt: Point): void {
    const face = faceAt(pt);
    if (face < 0) return;
    this.#selected = face;
    this.#paint();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.#request.confirm(this.#selected);
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
