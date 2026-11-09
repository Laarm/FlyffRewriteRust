// `WndCreateChar` (`CWndCreateChar`, `src:_Interface/WndTitle.cpp:2397-3025`, ref/22 §17 B, Plan/42 §4.5, §8.12, 45 §5):
// `APP_CREATE_CHAR` (544 x 368, `WndTile08`). Name edit, sex buttons, hair-style and face arrows, the (disabled) hair-colour
// arrows, the bank PIN and its confirmation (masked, digits only, 4 characters) and OK / Cancel. The 3-D preview of the
// new character fills `(280, 0, 550, 305)` of the client area (the viewport of `OnDraw`); its state (sex, hair mesh,
// head mesh) lives in the controller (`CharCreateView.subject`), the shell only turns clicks into `CharCreateActions`.

import {
  APP_CREATE_CHAR,
  WIDC_EDIT_2ND_PASSWORD,
  WIDC_EDIT_2ND_PASSWORD_CONFIRM,
  WIDC_FACE_LEFT,
  WIDC_FACE_RIGHT,
  WIDC_FEMALE,
  WIDC_HAIRCOLOR_LEFT,
  WIDC_HAIRCOLOR_RIGHT,
  WIDC_HAIRSTYLE_LEFT,
  WIDC_HAIRSTYLE_RIGHT,
  WIDC_MALE,
  WIDC_NAME,
  WIDC_OK,
  WIDC_CANCEL,
  WIDC_STATIC7,
  WIDC_STATIC8,
  WIDC_STATIC_2ND_PASSWORD_TEXT,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { Rect } from '../../layout/geometry';
import { EBS_NUMBER, EBS_PASSWORD, WBS_KEY, WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type {
  CharCreateActions,
  CharCreateView,
  PreviewHandleLike,
  PreviewPort,
  ViewSource,
} from './ports';

/** `MAX_2ND_PASSWORD_NUMBER`. */
export const PIN_LENGTH = 4;

/** The viewport of `OnDraw`: x 280, y 0, `550 - 280` wide, `320 - 15` high (client coordinates). */
export const CREATE_PREVIEW_RECT: Rect = { l: 280, t: 0, r: 550, b: 305 };

/** `D3DCOLOR_ARGB(255, 255, 0, 0)`: the colour of the PIN note. */
export const PIN_NOTE_COLOR = 0xffff0000;

export class WndCreateChar extends WndNeuz {
  readonly appId = APP_CREATE_CHAR;
  readonly #model: ViewSource<CharCreateView>;
  readonly #actions: CharCreateActions;
  readonly #previews: PreviewPort | null;
  #handle: PreviewHandleLike | null = null;
  #subject: unknown = undefined;
  #previewSex = -1;
  #name: EditCtrl | null = null;
  #pin: EditCtrl | null = null;
  #pinConfirm: EditCtrl | null = null;
  #ok: Button | null = null;
  #cancel: Button | null = null;
  #off: (() => void) | null = null;

  constructor(
    model: ViewSource<CharCreateView>,
    actions: CharCreateActions,
    previews: PreviewPort | null = null,
  ) {
    super();
    this.#model = model;
    this.#actions = actions;
    this.#previews = previews;
    this.putRegInfo = false; // `SetPutRegInfo(FALSE)`
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_KEY | extraStyle);
  }

  get nameEdit(): EditCtrl | null {
    return this.#name;
  }
  get pinEdit(): EditCtrl | null {
    return this.#pin;
  }
  get pinConfirmEdit(): EditCtrl | null {
    return this.#pinConfirm;
  }
  get okButton(): Button | null {
    return this.#ok;
  }
  get cancelButton(): Button | null {
    return this.#cancel;
  }
  get preview(): PreviewHandleLike | null {
    return this.#handle;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // the hair colour is fixed by the shared rule: the arrows exist but are disabled (legacy)
    this.getDlgItem<Button>(WIDC_HAIRCOLOR_LEFT)?.enableWindow(false);
    this.getDlgItem<Button>(WIDC_HAIRCOLOR_RIGHT)?.enableWindow(false);
    // the two empty tag statics of the Thai variant
    this.getDlgItem<Static>(WIDC_STATIC7)?.enableWindow(false);
    this.getDlgItem<Static>(WIDC_STATIC8)?.enableWindow(false);
    const ok = this.getDlgItem<Button>(WIDC_OK);
    ok?.setDefault(true);
    this.#ok = ok;
    this.#cancel = this.getDlgItem<Button>(WIDC_CANCEL);
    const m = this.#model.get();
    const name = this.getDlgItem<EditCtrl>(WIDC_NAME);
    this.#name = name;
    name?.setTabStop(true);
    name?.setMaxLength(m.maxNameLength);
    // bank PIN: masked, digits only, 4 characters (`AddWndStyle(EBS_PASSWORD | EBS_AUTOHSCROLL | EBS_NUMBER)`)
    const pin = this.getDlgItem<EditCtrl>(WIDC_EDIT_2ND_PASSWORD);
    const confirm = this.getDlgItem<EditCtrl>(WIDC_EDIT_2ND_PASSWORD_CONFIRM);
    for (const e of [pin, confirm]) {
      e?.addWndStyle(EBS_PASSWORD | EBS_NUMBER);
      e?.setMaxStringNumber(PIN_LENGTH);
      e?.setTabStop(true);
    }
    this.#pin = pin;
    this.#pinConfirm = confirm;
    this.getDlgItem<Static>(WIDC_STATIC_2ND_PASSWORD_TEXT)?.setTextColor(PIN_NOTE_COLOR);
    this.moveParentCenter();
    this.#apply(m);
    this.#off = this.#model.subscribe(() => {
      this.#apply(this.#model.get());
    });
    name?.setFocus();
  }

  #apply(m: CharCreateView): void {
    this.#ok?.enableWindow(!m.busy);
    this.#cancel?.enableWindow(!m.busy);
    if (this.#previews === null) return;
    // another sex is another body (skeleton, motion `STAND` / `STAND2`): the preview is reopened
    if (this.#handle !== null && this.#previewSex !== m.sex) {
      this.#handle.close();
      this.#handle = null;
    }
    if (this.#handle === null) {
      const canvas = this.addCustomCanvas('PREVIEW', CREATE_PREVIEW_RECT);
      if (!canvas) return;
      this.#subject = m.subject;
      this.#previewSex = m.sex;
      this.#handle = this.#previews.open({ canvas, preset: 'charCreate', subject: m.subject });
      return;
    }
    if (this.#subject !== m.subject) {
      this.#subject = m.subject;
      this.#handle.setSubject(m.subject);
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const busy = this.#model.get().busy;
      switch (id) {
        case WIDC_MALE:
          this.#actions.setSex(0);
          return true;
        case WIDC_FEMALE:
          this.#actions.setSex(1);
          return true;
        case WIDC_HAIRSTYLE_LEFT:
          this.#actions.stepHair(-1);
          return true;
        case WIDC_HAIRSTYLE_RIGHT:
          this.#actions.stepHair(1);
          return true;
        case WIDC_FACE_LEFT:
          this.#actions.stepHead(-1);
          return true;
        case WIDC_FACE_RIGHT:
          this.#actions.stepHead(1);
          return true;
        case WTBID_CLOSE:
        case WIDC_CANCEL:
          if (!busy) this.#actions.cancel();
          return true;
        case WIDC_OK:
          if (!busy) this.#submit();
          return true;
        default:
          break;
      }
    }
    if (msg === Wnm.EN_RETURN && id === WIDC_NAME) {
      if (!this.#model.get().busy) this.#submit();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  #submit(): void {
    this.#actions.ok(
      this.#name?.getString() ?? '',
      this.#pin?.getString() ?? '',
      this.#pinConfirm?.getString() ?? '',
    );
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#handle?.close();
    this.#handle = null;
    super.onDestroy();
  }
}
