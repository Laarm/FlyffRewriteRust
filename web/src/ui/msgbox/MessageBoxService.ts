// `MessageBoxApi` (Plan/42 §14; `CWndMgr::OpenMessageBox*`, WndManager.cpp:2288-2347): opens the standard dialogs on the
// window manager. A new message box REPLACES the current one (`SAFE_DELETE(m_pWndMessageBox)`); the upper box is an
// independent second one; the quantity and confirm boxes replace their own previous instance.

import { APP_COMMITEM_DIALOG, APP_MESSAGEBOX, APP_TRADE_GOLD } from '../../gen/defs/resdata';
import type { WindowManager } from '../wm/WindowManager';
import type { WndNeuz } from '../wnd/WndNeuz';
import { ConfirmBoxWnd } from './ConfirmBox';
import { MessageBoxWnd } from './MessageBoxWnd';
import { QuantityBoxWnd, type QuantityLabels } from './QuantityBox';
import { MB, type MbType } from './types';

export interface MessageBoxApi {
  /** `OpenMessageBox`: replaces the current box; a non-root `parent` receives `onCommand(id)` and `onResult(id)` follows. */
  open(
    text: string,
    type?: MbType,
    parent?: WndNeuz,
    onResult?: (id: number) => void,
  ): MessageBoxWnd;
  /** `OpenMessageBoxUpper`: an independent second box near the top of the screen (`postLogout` asks the host to log out). */
  openUpper(text: string, type?: MbType, postLogout?: boolean): MessageBoxWnd;
  /** `OpenMessageBoxWithTitle`. */
  openWithTitle(title: string, text: string, type?: MbType): MessageBoxWnd;
  /** `OpenCustomBox`: a box class of a window batch (Quit, LogOut, ...) replaces the current box. */
  openCustom(box: MessageBoxWnd): void;
  /** The quantity box on `APP_TRADE_GOLD`: OK gives `[1, max]`; `labels` replace the two captions of the layout (the question and the label of the edit). */
  quantity(
    max: number,
    onOk: (n: number) => void,
    title?: string,
    onCancel?: () => void,
    labels?: QuantityLabels,
  ): void;
  /** The confirm box on `APP_COMMITEM_DIALOG`: Yes / No (Enter = Yes). */
  confirm(text: string, onYes: () => void, onNo?: () => void): void;
  /** `CloseMessageBox`: drops both message boxes (and the quantity and confirm boxes). */
  closeAll(): void;
}

const MAIN = 'main';
const UPPER = 'upper';
const QUANTITY = 'quantity';
const CONFIRM = 'confirm';

export class MessageBoxService implements MessageBoxApi {
  readonly #wm: WindowManager;

  constructor(wm: WindowManager) {
    this.#wm = wm;
  }

  /** Destroys the instance of `app`/`key` at once so that `open` builds a fresh one. */
  #replace(app: number, key: string): void {
    const old = this.#wm.get(app, key);
    if (!old) return;
    this.#wm.destroy(old);
    this.#wm.flushDestroyed();
  }

  #show<T extends WndNeuz>(box: T, key: string): T {
    this.#replace(box.appId, key);
    this.#wm.open(box.appId, () => box, key);
    return box;
  }

  open(
    text: string,
    type: MbType = MB.OK,
    parent?: WndNeuz,
    onResult?: (id: number) => void,
  ): MessageBoxWnd {
    const box = new MessageBoxWnd(text, type);
    if (onResult) box.onResult = onResult;
    // the box itself is a frame of the root (`pWndParent = this`); an owner gets the command
    if (parent) box.owner = parent;
    return this.#show(box, MAIN);
  }

  openUpper(text: string, type: MbType = MB.OK, postLogout = false): MessageBoxWnd {
    const box = new MessageBoxWnd(text, type, true);
    box.postLogout = postLogout;
    return this.#show(box, UPPER);
  }

  openWithTitle(title: string, text: string, type: MbType = MB.OK): MessageBoxWnd {
    const box = new MessageBoxWnd(text, type);
    box.setBoxTitle(title);
    return this.#show(box, MAIN);
  }

  openCustom(box: MessageBoxWnd): void {
    this.#show(box, MAIN);
  }

  quantity(
    max: number,
    onOk: (n: number) => void,
    title?: string,
    onCancel?: () => void,
    labels?: QuantityLabels,
  ): void {
    this.#show(new QuantityBoxWnd(max, onOk, title, onCancel, labels), QUANTITY);
  }

  confirm(text: string, onYes: () => void, onNo?: () => void): void {
    this.#show(new ConfirmBoxWnd(text, onYes, onNo), CONFIRM);
  }

  closeAll(): void {
    this.#replace(APP_MESSAGEBOX, MAIN);
    this.#replace(APP_MESSAGEBOX, UPPER);
    this.#replace(APP_TRADE_GOLD, QUANTITY);
    this.#replace(APP_COMMITEM_DIALOG, CONFIRM);
  }
}
