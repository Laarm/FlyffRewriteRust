// `WndFastJobChange` (`CWndJobChangeEx`, `src:_Interface/WndChangeJobEx.cpp`, ref/22 §17 S, Plan/42 §19.4 S, 45 §21; WP-UI-38):
// `APP_FASTJOBCHANGE` "Select Class" (432 x 288, `features.fast_jobchange`). One class at a time: its picture (`Slot<Job>.bmp`) on
// the big button, its name above, its legacy description below; the arrows (`WIDC_BUTTON3` left, `WIDC_BUTTON2` right) step through
// the classes on offer and are disabled at the ends; "Select Class" (`WIDC_BUTTON1`) applies the shown class and closes. With
// nothing on offer the window is in its error state: the vagrant picture disabled, the title "Error" with the tooltip
// "You don't have the right level", every button disabled. The window is centred when it opens.

import { APP_FASTJOBCHANGE } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { ERROR_PICTURE, jobPicture } from './jobPictures';
import { ERROR_TITLE, ERROR_TOOLTIP } from './fastJobTexts';
import type { FastJobRequest } from './ports';

/** The legacy texts of the error state. */
export { ERROR_TITLE, ERROR_TOOLTIP };

export class WndFastJobChange extends WndNeuz {
  readonly appId = APP_FASTJOBCHANGE;
  readonly #request: FastJobRequest;
  /** Index into `targets` of the class shown (`nCurJob - nJobMin`). */
  #cursor = 0;

  constructor(request: FastJobRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** The class shown, `null` in the error state. */
  get current(): number | null {
    return this.#request.targets[this.#cursor] ?? null;
  }
  get applyButton(): Button | null {
    return this.getDlgItem<Button>('WIDC_BUTTON1');
  }
  /** `WIDC_BUTTON2`: the next class (›). */
  get nextButton(): Button | null {
    return this.getDlgItem<Button>('WIDC_BUTTON2');
  }
  /** `WIDC_BUTTON3`: the previous class (‹). */
  get prevButton(): Button | null {
    return this.getDlgItem<Button>('WIDC_BUTTON3');
  }
  /** `WIDC_BUTTON4`: the picture. */
  get pictureButton(): Button | null {
    return this.getDlgItem<Button>('WIDC_BUTTON4');
  }
  get titleStatic(): Static | null {
    return this.getDlgItem<Static>('WIDC_STATIC1');
  }
  get info(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>('WIDC_TEXT1');
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#show();
    this.moveParentCenter();
  }

  /** `OnDraw`: the picture, the title, the description and the arrows for the class under the cursor. */
  #show(): void {
    const job = this.current;
    if (job === null) {
      this.pictureButton?.setTexture(ERROR_PICTURE);
      this.pictureButton?.enableWindow(false);
      this.titleStatic?.setToolTip(ERROR_TOOLTIP);
      this.titleStatic?.setTitle(ERROR_TITLE);
      this.applyButton?.enableWindow(false);
      this.nextButton?.enableWindow(false);
      this.prevButton?.enableWindow(false);
      return;
    }
    const pic = jobPicture(job);
    if (pic !== null) this.pictureButton?.setTexture(pic);
    const name = this.#request.jobName(job);
    this.pictureButton?.setToolTip(name);
    this.titleStatic?.setTitle(name);
    this.titleStatic?.setToolTip('');
    this.applyButton?.enableWindow(true);
    const last = this.#request.targets.length - 1;
    this.nextButton?.enableWindow(this.#cursor < last);
    this.prevButton?.enableWindow(this.#cursor > 0);
    this.info?.setString(this.#request.description(job));
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === this.applyButton?.id) {
        const job = this.current;
        if (job !== null) {
          this.#request.apply(job);
          this.destroy();
        }
        return true;
      }
      if (id === this.nextButton?.id) {
        if (this.#cursor < this.#request.targets.length - 1) this.#cursor += 1;
        this.#show();
        return true;
      }
      if (id === this.prevButton?.id) {
        if (this.#cursor > 0) this.#cursor -= 1;
        this.#show();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
