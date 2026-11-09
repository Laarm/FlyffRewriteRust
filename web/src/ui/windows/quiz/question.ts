// What the two question windows share (`CWndQuizEventQuestionOX` / `...4C`, `src:_Interface/WndQuizEvent.cpp:78-261`; WP-UI-59): opening closes the inventory (the window's own `OnInitialUpdate`), `CUSTOM1` shows the picture
// `char/char_Juria_JAP.tga`, `TEXT1` the question in black, the window is centred; an answer reports and closes, `CLOSE` closes, and a window that goes without an answer reports `onClose`.

import { APP_INVENTORY, WIDC_CLOSE, WIDC_CUSTOM1, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { MISSING } from '../../skin/images';
import { Wnm } from '../../layout/styles';
import { rectHeight, rectWidth } from '../../layout/geometry';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';

/** The picture of the instructor. */
export const QUIZ_CHAR_PICTURE = 'char/char_juria_jap.tga';
/** `0xff000000`: the colour of the question text. */
export const QUESTION_COLOR = 0xff000000;

export abstract class QuizQuestionWnd extends WndNeuz {
  #answered = false;
  #closeReported = false;
  #picture: HTMLCanvasElement | null = null;
  #image: { width: number; height: number; data: Uint8ClampedArray | Uint8Array } | null = null;
  #question: string;

  protected constructor(
    question: string,
    private readonly reportClose: (() => void) | undefined,
  ) {
    super();
    this.#question = question;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  /** The question now shown. */
  get question(): string {
    return this.#question;
  }
  /** The instructor's picture was decoded (tests). */
  get pictureReady(): boolean {
    return this.#image !== null;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `GetApplet( APP_INVENTORY )->Destroy()`: the question window closes the bag
    this.env.wm.close(APP_INVENTORY);
    this.text?.setString(this.#question, QUESTION_COLOR);
    this.#picture = this.customCanvas(WIDC_CUSTOM1);
    void this.#loadPicture();
    this.moveParentCenter();
  }

  async #loadPicture(): Promise<void> {
    const img = await this.env.images.get(QUIZ_CHAR_PICTURE, 'exact');
    if (this.destroyed || img === MISSING) return;
    this.#image = img;
    this.invalidate(WIDC_CUSTOM1);
  }

  /** `OnDraw`: the picture scaled to the placeholder (`m_texChar.Render( TopLeft, BottomRight )`). */
  override onDraw(): void {
    const canvas = this.#picture;
    const img = this.#image;
    if (!canvas || !img || typeof document === 'undefined') return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const off = document.createElement('canvas');
    off.width = img.width;
    off.height = img.height;
    const octx = off.getContext('2d');
    if (!octx) return;
    octx.putImageData(new ImageData(new Uint8ClampedArray(img.data), img.width, img.height), 0, 0);
    const r = this.getWndCtrl(WIDC_CUSTOM1)?.rect;
    if (r) ctx.drawImage(off, 0, 0, rectWidth(r), rectHeight(r));
  }

  /** `UpdateQuestion( text )`. */
  update(question: string): void {
    this.#question = question;
    this.text?.setString(question, QUESTION_COLOR);
  }

  /** Reports the answer and closes. */
  protected answered(report: () => void): void {
    this.#answered = true;
    report();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_CLOSE) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    if (!this.#answered && !this.#closeReported) {
      this.#closeReported = true;
      this.reportClose?.();
    }
    super.onDestroy();
  }
}
