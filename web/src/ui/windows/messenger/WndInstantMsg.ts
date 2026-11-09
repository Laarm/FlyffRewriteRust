// `WndInstantMsg` (`CWndInstantMsg`, `src:_Interface/WndMessenger.cpp:960-1100`, Plan/42 §19.4 I, 45 §24.1; WP-UI-44): `APP_INSTANTMSG` (144 x 112, the picture
// `WndInstantMsg.tga`), the popup of an instant message, of a friend who logged in, of a mail (`Mail`) and of an event (`Event`). It opens without taking the focus
// (`WBS_NOFOCUS`: a player typing keeps typing), sits at the bottom right, collects lines in its text control and vanishes `ttlMs` after the last one. A click
// (not on the close button) is the request's: the IM window opens with the LAST line and the popup closes - or nothing happens for `Mail`.

import { APP_INSTANTMSG, WIDC_TEXT } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NOFOCUS, WTBID_CLOSE } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { DEFAULT_TEXT_COLOR } from '../../text/markup';
import type { InstantLine, InstantRequest } from './ports';

export class WndInstantMsg extends WndNeuz {
  readonly appId = APP_INSTANTMSG;
  override instanceKey: string;
  readonly #request: InstantRequest;
  #key: string;
  #text: TextCtrl | null = null;
  /** `m_timer`: the UI clock at which the popup ends; reset by every line. */
  #endsAt = 0;

  constructor(request: InstantRequest) {
    super();
    this.#request = request;
    this.#key = request.key;
    this.instanceKey = request.key;
    this.putRegInfo = false;
  }

  get key(): string {
    return this.#key;
  }
  /** The window manager re-keyed the popup (`renameInstant`): the key follows. */
  setKey(key: string): void {
    this.#key = key;
    this.instanceKey = key;
  }
  get text(): TextCtrl | null {
    return this.#text;
  }
  /** The UI clock at which the popup ends. */
  get endsAt(): number {
    return this.#endsAt;
  }

  /** `InitDialog( ..., WBS_NOFOCUS )`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, extraStyle | WBS_NOFOCUS);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT);
    this.#append(this.#request.line);
    this.#endsAt = this.env.host.now() + this.#request.ttlMs;
    // `Move( rectRoot.right - width, rectRoot.bottom - height )`: the service stacks the popups afterwards
    const root = this.env.wm.layoutRect;
    this.move({
      x: root.r - rectWidth(this.rectWindow),
      y: root.b - rectHeight(this.rectWindow),
    });
  }

  /** `AddMessage` / `AddMessageJoin` / `AddPostMessage`: another line, and the timer starts again. */
  addText(line: InstantLine): void {
    this.#append(line);
    this.#endsAt = this.env.host.now() + this.#request.ttlMs;
  }

  /** The head is rich text, the sender's words are text (`PS_NOT_MACRO`). */
  #append(line: InstantLine): void {
    if (line.head !== '') this.#text?.addString(line.head);
    if (line.body !== '') this.#text?.addString(line.body, DEFAULT_TEXT_COLOR, false);
  }

  /** The popup's rich text (tests). */
  get plainText(): string {
    return this.#text?.getString() ?? '';
  }

  override process(now: number): void {
    // `if( m_timer.IsTimeOut() ) Destroy()`; the clock of the window manager is the host clock
    if (now >= this.#endsAt) {
      this.destroy();
      return;
    }
    super.process(now);
  }

  /** A click on the popup: the request decides; `true` closes it. */
  click(): void {
    if (this.#request.click?.() === true) this.destroy();
  }

  override onLButtonUp(): void {
    this.click();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    // the text control reports its click (`STN_CLICKED`) to the popup: any notification but the close button is a click
    if (id !== WTBID_CLOSE && id === (this.#text?.id ?? -1)) {
      this.click();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
