// `WndChangeSex` (`CWndChangeSex`, `src:_Interface/WndChangeFace.cpp:131-330`, ref/22 §17 L, Plan/42 §19.4 L, 45 §18.4; WP-UI-35 / WP-CFEAT-15):
// `APP_CHANGESEX` (192 x 272), the scroll of sex change (`II_CHR_SYS_SCR_TRANSY`, rule R20). `WIDC_STATIC1` (150 x 130) frames a 3-D preview of the
// OTHER sex (the player's hair and equipment on the model of the opposite sex, `MTI_STAND2`, doc 41 preset `changeSex`; the viewport of the
// original starts 6 px left of the rect) and `WIDC_LEFT` / `WIDC_RIGHT` page through the five faces (wrapping), each page re-assembling the preview
// (`CMover::UpdateParts( !sex, 0, 0, hair, face, ... )`). OK sends `DoUseItem(scroll, target, face)` and closes; Cancel and the close button close.

import {
  APP_CHANGESEX,
  WIDC_CANCEL,
  WIDC_LEFT,
  WIDC_OK,
  WIDC_RIGHT,
  WIDC_STATIC1,
} from '../../../gen/defs/resdata';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { PreviewHandleLike } from '../login/ports';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PREVIEW_SHIFT } from './geometry';
import { CHANGE_FACE_COUNT, type ChangeSexRequest } from './ports';

export { PREVIEW_SHIFT };

export class WndChangeSex extends WndNeuz {
  readonly appId = APP_CHANGESEX;
  readonly #request: ChangeSexRequest;
  #selected = 0;
  #handle: PreviewHandleLike | null = null;

  constructor(request: ChangeSexRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `m_nSelectFace`. */
  get selected(): number {
    return this.#selected;
  }
  get preview(): PreviewHandleLike | null {
    return this.#handle;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const preview = this.#request.preview;
    const rect = this.getWndCtrl(WIDC_STATIC1)?.rect;
    if (preview && rect) {
      const canvas = this.addCustomCanvas(
        'changesex.preview',
        { l: rect.l - PREVIEW_SHIFT, t: rect.t, r: rect.r - PREVIEW_SHIFT, b: rect.b },
        true,
      );
      if (canvas) {
        this.#handle = preview.port.open({
          canvas,
          preset: 'changeSex',
          subject: preview.subject(this.#selected),
          animate: false,
        });
      }
    }
    this.moveParentCenter();
  }

  /** `WIDC_LEFT` / `WIDC_RIGHT`: the previous / next face, wrapping over the five. */
  step(delta: -1 | 1): void {
    this.#selected = (this.#selected + delta + CHANGE_FACE_COUNT) % CHANGE_FACE_COUNT;
    const preview = this.#request.preview;
    if (preview) this.#handle?.setSubject(preview.subject(this.#selected));
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_OK:
          this.#request.confirm(this.#selected);
          this.destroy();
          return true;
        case WIDC_CANCEL:
          this.destroy();
          return true;
        case WIDC_LEFT:
          this.step(-1);
          return true;
        case WIDC_RIGHT:
          this.step(1);
          return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#handle?.close();
    this.#handle = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
