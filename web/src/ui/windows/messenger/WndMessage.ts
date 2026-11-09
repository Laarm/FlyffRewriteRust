// `WndMessage` (`CWndMessage`, `src:_Interface/WndMessenger.cpp:1100-1260`, Plan/42 §19.4 I, 45 §8.3, §24.1; WP-UI-43): `APP_MESSAGE` (336 x 288, resizable, minimum
// 160 x 160), the instant-message window of ONE partner (`instanceKey` = the lower-cased name: two partners have two windows, a second open of one partner
// focuses the first). The text shows the conversation as rich text - the own lines in red, the partner's in blue, each `<from><TID_GAME_FROM3> :` and then the
// text indented by two blanks - and follows the end; the edit below takes the next line, Enter or Send hand it to the request (`/say "<name>" <text>` is the feature's).
// There is no local echo: the own line comes back from the server. A blocked partner hides the edit and disables Send (FIX: the original did so only when an echo came back).
//
// `OnSize`: the text is the client deflated by 4 / 4 / 4 / 50, the edit runs from `bottom - 45` to `bottom - 4` and from the left 4 to `right - 80`, Send sits at `right - 74`.

import { APP_MESSAGE, WIDC_EDIT, WIDC_SEND, WIDC_TEXT } from '../../../gen/defs/resdata';
import { TID_GAME_FROM3 } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type FrameRects, type Rect, rectHeight, rectWidth } from '../../layout/geometry';
import { EBS_AUTOVSCROLL, EBS_MULTILINE, WBS_THICKFRAME, Wnm } from '../../layout/styles';
import { adjustMinRect } from '../../wm/dragMath';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { imKey } from './imKey';
import type { ImLine, ImRequest, ImView } from './ports';

/** `AdjustMinRect( &rectWnd, 16 * 10, 16 * 10 )`. */
export const IM_MIN = { w: 160, h: 160 } as const;

/** The rich text of a line: `#cffff0000<from><TID_GAME_FROM3> :#nc` and the text on the next line. */
export function imHeader(line: ImLine, from3: string): string {
  const color = line.mine ? 'ffff0000' : 'ff0000ff';
  return `#c${color}${line.from}${from3} :#nc\n  `;
}

export class WndMessage extends WndNeuz {
  readonly appId = APP_MESSAGE;
  override instanceKey: string;
  readonly #request: ImRequest;
  readonly #getText: (tid: number) => string;
  #view: ImView;
  #text: TextCtrl | null = null;
  #edit: EditCtrl | null = null;
  #send: Button | null = null;
  #unsubscribe: (() => void) | null = null;
  /** The lines already in the text control (appended lines are added, anything else rebuilds it). */
  #shown: readonly ImLine[] = [];
  #title = '';

  constructor(request: ImRequest, getText: (tid: number) => string) {
    super();
    this.#request = request;
    this.#getText = getText;
    this.#view = request.view.get();
    this.instanceKey = imKey(this.#view.partner);
    this.putRegInfo = false;
  }

  get partner(): string {
    return this.#view.partner;
  }
  /** The window manager re-keyed the window (`renameMessage`): the key follows. */
  setKey(key: string): void {
    this.instanceKey = key;
  }
  get text(): TextCtrl | null {
    return this.#text;
  }
  get edit(): EditCtrl | null {
    return this.#edit;
  }
  get sendButton(): Button | null {
    return this.#send;
  }
  /** The lines in the text control (tests). */
  get lines(): readonly ImLine[] {
    return this.#shown;
  }
  /** `GetDlgItem( WIDC_EDIT )->IsVisible()` and `WIDC_SEND` enabled: the window takes input. */
  get enabledInput(): boolean {
    return (this.#edit?.isVisible() ?? false) && (this.#send?.isWindowEnabled() ?? false);
  }

  /** `InitDialog( ..., WBS_THICKFRAME )`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, extraStyle | WBS_THICKFRAME);
  }

  /** `CWndMessage::SetWndRect`: `AdjustMinRect( 160, 160 )`. */
  protected override computeRects(window: Rect): FrameRects {
    return super.computeRects(adjustMinRect(window, this.rectWindowValue, IM_MIN.w, IM_MIN.h));
  }

  override constrainResize(next: Rect, current: Rect): Rect {
    return adjustMinRect(next, current, IM_MIN.w, IM_MIN.h);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT);
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT);
    this.#send = this.getDlgItem<Button>(WIDC_SEND);
    // `pWndEdit->AddWndStyle( EBS_AUTOVSCROLL )` + `SetNativeMode()`: a wrapping edit whose Enter sends
    this.#edit?.addWndStyle(EBS_MULTILINE | EBS_AUTOVSCROLL);
    this.#edit?.setMaxLength(this.#request.maxChars);
    this.#title = this.getTitle();
    this.#retitle(this.#view.partner);
    this.#layout();
    this.#apply(this.#request.view.get());
    this.#unsubscribe = this.#request.view.subscribe(() => {
      this.#apply(this.#request.view.get());
    });
    this.#edit?.setFocus();
    this.moveParentCenter();
  }

  override onSize(type: number, cx: number, cy: number): void {
    this.#layout();
    super.onSize(type, cx, cy);
  }

  /** `CWndMessage::OnSize`. */
  #layout(): void {
    const client = this.getClientRect(false);
    const w = rectWidth(client);
    const h = rectHeight(client);
    this.#text?.setWndRect({ l: 4, t: 4, r: w - 4, b: h - 50 }, true);
    this.#edit?.setWndRect({ l: 4, t: h - 45, r: w - 80, b: h - 4 }, true);
    const send = this.#send;
    if (send) {
      const sw = rectWidth(send.rectWindow);
      const sh = rectHeight(send.rectWindow);
      const x = w - 74;
      const y = h - 45;
      send.setWndRect({ l: x, t: y, r: x + sw, b: y + sh }, true);
    }
  }

  // ---- the conversation ---------------------------------------------------------------------------------------------------------

  #apply(v: ImView): void {
    const previous = this.#view;
    this.#view = v;
    if (v.partner !== previous.partner) this.#retitle(v.partner);
    this.#syncLines(v.lines);
    const blocked = v.blocked;
    this.#edit?.setVisible(!blocked);
    this.#send?.enableWindow(!blocked);
  }

  #retitle(partner: string): void {
    this.setTitle(`${this.#title} - ${partner}`);
  }

  #syncLines(lines: readonly ImLine[]): void {
    const text = this.#text;
    if (!text) return;
    const shown = this.#shown;
    const append = lines.length >= shown.length && shown.every((l, i) => lines[i] === l);
    if (!append) text.setString('');
    const from = append ? shown.length : 0;
    const from3 = this.#getText(TID_GAME_FROM3);
    for (let i = from; i < lines.length; i += 1) {
      const line = lines[i];
      if (!line) continue;
      text.addString(imHeader(line, from3));
      // the partner's words are text, not markup
      text.addString(`${line.text}\n`, 0xff000000, false);
    }
    this.#shown = lines;
    if (lines.length !== from) text.scrollToEnd();
  }

  // ---- sending -------------------------------------------------------------------------------------------------------------------

  /** Enter in the edit and the Send button: a non-empty text goes to the request and the edit empties. */
  submit(): boolean {
    const edit = this.#edit;
    if (!edit || this.#view.blocked) return false;
    const raw = edit.getString();
    if (raw === '') return false;
    this.#request.send(raw.replace(/\s*[\r\n]+\s*/g, ' '));
    edit.empty();
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_EDIT && msg === Wnm.EN_RETURN) {
      this.submit();
      return true;
    }
    if (id === WIDC_SEND && msg === Wnm.CLICKED) {
      this.submit();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
