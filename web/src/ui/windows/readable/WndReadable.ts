// The three readable-item windows (`CWndTextBook` 432 x 336, `CWndTextScroll` and `CWndTextLetter` 256 x 368, `src:_Interface/WndText.cpp`, ref/22 §17 D,
// Plan/42 §19.4 D; WP-UI-37). Each is a picture of the object (`subBook.tga`, `subScroll.tga`, `subLetter.tga`, 32-bit A8R8G8B8 at full precision:
// painted over the baked frame in the `exact` key mode, not the quantised `nibble` one) with the item's text in `WIDC_TEXT1`, an Accept button
// (`WIDC_ACCEPT`, disabled unless the item starts a quest) and a Close button (`WIDC_CLOSE`). The book has a second page control (`WIDC_TEXT2`) and two
// arrows that the original never fills or wires: they stay empty here too. The windows open centred.

import {
  APP_TEXT_BOOK,
  APP_TEXT_LETTER,
  APP_TEXT_SCROLL,
  WIDC_ACCEPT,
  WIDC_CLOSE,
  WIDC_STATIC1,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { READABLE_PICTURE } from './constants';
import type { ReadableRequest } from './ports';

abstract class WndReadable extends WndNeuz {
  readonly #request: ReadableRequest;
  #text: TextCtrl | null = null;

  protected constructor(
    request: ReadableRequest,
    private readonly picture: string,
  ) {
    super();
    this.#request = request;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }
  get acceptButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_ACCEPT);
  }
  get closeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CLOSE);
  }
  get request(): ReadableRequest {
    return this.#request;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#request.text);
    this.acceptButton?.enableWindow(this.#request.acceptable);
    // the picture of the object at its place in `WIDC_STATIC1`, A8R8G8B8 at full precision
    const at = this.getWndCtrl(WIDC_STATIC1)?.rect;
    if (at) {
      this.afterSkinImages = [{ path: this.picture, mode: 'exact' }];
      this.afterSkin = (ctx) => {
        ctx.blit(
          this.picture,
          at.l + this.rectClient.l - this.rectWindow.l,
          at.t + this.rectClient.t - this.rectWindow.t,
          'exact',
        );
      };
      this.requestBake();
    }
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_ACCEPT) {
        if (this.#request.acceptable) this.#request.accept();
        return true;
      }
      if (id === WIDC_CLOSE) {
        this.destroy();
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

export class WndTextBook extends WndReadable {
  readonly appId = APP_TEXT_BOOK;
  constructor(request: ReadableRequest) {
    super(request, READABLE_PICTURE.book);
  }
}

export class WndTextScroll extends WndReadable {
  readonly appId = APP_TEXT_SCROLL;
  constructor(request: ReadableRequest) {
    super(request, READABLE_PICTURE.scroll);
  }
}

export class WndTextLetter extends WndReadable {
  readonly appId = APP_TEXT_LETTER;
  constructor(request: ReadableRequest) {
    super(request, READABLE_PICTURE.letter);
  }
}
