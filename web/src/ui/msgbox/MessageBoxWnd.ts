// `MessageBoxWnd` (`CWndMessageBox` / `CWndMessageBoxUpper`, WndMessageBox.cpp; Plan/42 §14, ref/22 §13): the modal box
// on `APP_MESSAGEBOX` (256 x 144, `WndMessagebox.tga`, not tiled). The text is a `TextCtrl` with `WBS_VSCROLL` over the
// client rect deflated `(10, 10, 10, 35)`; the buttons are 60 x 25 at `y = clientH - 30`, centred (`OK` and `CANCEL`
// use the sprites `ButtOk.tga` / `ButtCancel.tga` and take the 72 px width of one frame); button 1 is the default
// (Enter, through the `WBS_KEY` handling). Any button destroys the box, tells a non-root parent
// (`parent.onCommand(id)`) and the caller (`onResult(id)`). A box whose text is `TID_DIAG_0035` sends the client back
// to the login state (the original exited the program).

import { APP_MESSAGEBOX } from '../../gen/defs/resdata';
import { TID_DIAG_0035, TID_GAME_CANCEL, TID_GAME_NO, TID_GAME_YES } from '../../gen/defs/text';
import { Button } from '../controls/Button';
import { TextCtrl } from '../controls/TextCtrl';
import type { Rect } from '../layout/geometry';
import { rectHeight, rectWidth } from '../layout/geometry';
import { WBS_KEY, WBS_MODAL, WBS_NODRAWFRAME, WBS_VSCROLL, Wnm } from '../layout/styles';
import type { Shortcut } from '../dnd/shortcut';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import { MB, MBID, type MbType } from './types';

/** Captions of the buttons that have no texture (English where the texts bundle has no string). */
const CAPTIONS: Readonly<Record<number, { tid?: number; en: string }>> = {
  [MBID.OK]: { en: 'OK' },
  [MBID.CANCEL]: { tid: TID_GAME_CANCEL, en: 'Cancel' },
  [MBID.ABORT]: { en: 'Abort' },
  [MBID.RETRY]: { en: 'Retry' },
  [MBID.IGNORE]: { en: 'Ignore' },
  [MBID.YES]: { tid: TID_GAME_YES, en: 'Yes' },
  [MBID.NO]: { tid: TID_GAME_NO, en: 'No' },
};

/** The buttons of a box type, left to right, as button ids. */
export function buttonsOf(type: MbType): readonly number[] {
  switch (type) {
    case MB.OK:
      return [MBID.OK];
    case MB.CANCEL:
      return [MBID.CANCEL];
    case MB.OKCANCEL:
      return [MBID.OK, MBID.CANCEL];
    case MB.ABORTRETRYIGNORE:
      return [MBID.ABORT, MBID.RETRY, MBID.IGNORE];
    case MB.YESNOCANCEL:
      return [MBID.YES, MBID.NO, MBID.CANCEL];
    case MB.YESNO:
      return [MBID.YES, MBID.NO];
    case MB.RETRYCANCEL:
      return [MBID.RETRY, MBID.CANCEL];
  }
}

/** The 60 x 25 button rects of `OnInitialUpdate` for `n` buttons in a client `clientW x clientH`. */
export function buttonRects(n: number, clientW: number, clientH: number): Rect[] {
  const x = Math.trunc(clientW / 2);
  const y = clientH - 30;
  const w = 60;
  const h = 25;
  const at = (left: number): Rect => ({ l: left, t: y, r: left + w, b: y + h });
  if (n === 1) return [at(x - Math.trunc(w / 2))];
  if (n === 2) return [at(x - w - 10), at(x + 10)];
  // three buttons: the original gave all three the first rect (a copy-paste slip, FIX)
  return [
    at(x - Math.trunc(w / 2) - w - 10),
    at(x - Math.trunc(w / 2)),
    at(x + Math.trunc(w / 2) + 10),
  ];
}

export class MessageBoxWnd extends WndNeuz {
  readonly appId: number = APP_MESSAGEBOX;
  readonly type: MbType;
  readonly message: string;
  /** The buttons by id, in creation order. */
  readonly buttons: Button[] = [];
  textCtrl: TextCtrl | null = null;
  /** A window other than the root that receives `onCommand(id)` when a button closes the box. */
  owner: WndBase | null = null;
  /** The caller's callback (`onResult(id)`). */
  onResult: ((id: number) => void) | null = null;
  /** `CWndMessageBoxUpper::m_bPostLogoutMsg`: a confirmed box asks the host to log out. */
  postLogout = false;
  /** Upper boxes sit at `((layout.r - w) / 2, 70)` instead of the centre. */
  readonly upper: boolean;
  #title: string | null = null;

  constructor(message: string, type: MbType = MB.OK, upper = false) {
    super();
    this.message = message;
    this.type = type;
    this.upper = upper;
  }

  /** A title other than the layout's (`OpenMessageBoxWithTitle`). */
  setBoxTitle(title: string): void {
    this.#title = title;
    this.setTitle(title);
  }

  /** `Initialize(message, parent, type)`: `InitDialog(APP_MESSAGEBOX, WBS_MODAL)` plus the key handling for Enter. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    if (this.#title !== null) this.setTitle(this.#title);
    const client = this.rectClient;
    const cw = rectWidth(client);
    const ch = rectHeight(client);
    // the text: client (0, 0, cw, ch) deflated (10, 10, 10, 35)
    const text = new TextCtrl();
    this.textCtrl = this.addControl(
      text,
      0,
      { l: 10, t: 10, r: cw - 10, b: ch - 35 },
      WBS_NODRAWFRAME | WBS_VSCROLL,
    );
    text.setString(this.message, 0xff000000);
    // the buttons
    const ids = buttonsOf(this.type);
    const rects = buttonRects(ids.length, cw, ch);
    ids.forEach((id, i) => {
      const rect = rects[i];
      if (!rect) return;
      const b = new Button();
      this.addControl(b, id, rect);
      b.setTitle(this.#caption(id));
      if (id === MBID.OK) b.setTexture('ButtOk.tga');
      else if (id === MBID.CANCEL && (this.type === MB.OKCANCEL || this.type === MB.CANCEL)) {
        b.setTexture('ButtCancel.tga');
      }
      if (b.state.get().texture !== null) void b.fitTextureSize();
      this.buttons.push(b);
    });
    this.buttons[0]?.setDefault(true);
    this.#place();
  }

  /** The caption of a button without a texture. */
  #caption(id: number): string {
    const c = CAPTIONS[id];
    if (!c) return '';
    const texts = this.env.host.bundles.texts;
    const t = c.tid !== undefined && texts ? texts.text(c.tid) : '';
    return t !== '' && t !== 'error' ? t : c.en;
  }

  /** `MoveParentCenter()` (normal boxes) or the upper position `((layout.r - w) / 2, 70)`. */
  #place(): void {
    if (!this.upper) {
      this.moveParentCenter();
      return;
    }
    const layout = this.env.wm.layoutRect;
    const w = rectWidth(this.rectWindow);
    this.move({ x: Math.trunc((layout.r - w) / 2), y: 70 });
  }

  /**
   * Every button destroys the box; a text equal to `TID_DIAG_0035` sends the client back to the login state; a parent
   * other than the root gets `onCommand(id)`; then the caller's `onResult(id)`.
   */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id >= MBID.OK && id <= MBID.NO) {
      const texts = this.env.host.bundles.texts;
      if (this.message === texts?.text(TID_DIAG_0035)) this.env.host.dialogs?.returnToLogin?.();
      if (this.postLogout) this.env.host.dialogs?.logout?.();
      this.destroy();
      const owner = this.owner ?? this.parent;
      if (owner && !owner.isRoot) owner.onCommand(id, msg, this);
      this.onResult?.(id);
    }
    return super.onChildNotify(msg, id, src);
  }
}
