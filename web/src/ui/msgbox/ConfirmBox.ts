// The confirm box (`CWndCommItemDlg` on `APP_COMMITEM_DIALOG`, WndField.cpp:13378-13470; Plan/42 §14): a read-only text
// with Yes (the layout's OK button) and No (Cancel). Enter = Yes (the frame handles keys, button OK is the default).

import { APP_COMMITEM_DIALOG } from '../../gen/defs/resdata';
import type { Button } from '../controls/Button';
import type { EditCtrl } from '../controls/EditCtrl';
import { EBS_MULTILINE, EBS_READONLY, WBS_KEY, WBS_MODAL, Wnm } from '../layout/styles';
import type { Shortcut } from '../dnd/shortcut';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';

export class ConfirmBoxWnd extends WndNeuz {
  readonly appId = APP_COMMITEM_DIALOG;
  readonly text: string;
  readonly #onYes: () => void;
  readonly #onNo: (() => void) | undefined;
  /** The Yes and No buttons (`WIDC_BUTTON_OK`, `WIDC_BUTTON_CANCEL`). */
  yes: Button | null = null;
  no: Button | null = null;

  constructor(text: string, onYes: () => void, onNo?: () => void) {
    super();
    this.text = text;
    this.#onYes = onYes;
    this.#onNo = onNo;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const edit = this.getDlgItem<EditCtrl>('WIDC_EDIT_COMMUSE');
    if (edit) {
      edit.addWndStyle(EBS_MULTILINE | EBS_READONLY); // a text area that only shows the question
      edit.setString(this.text);
    }
    this.yes = this.getDlgItem<Button>('WIDC_BUTTON_OK');
    this.no = this.getDlgItem<Button>('WIDC_BUTTON_CANCEL');
    this.yes?.setDefault(true);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === this.yes?.id) {
      this.destroy();
      this.#onYes();
      return true;
    }
    if (msg === Wnm.CLICKED && id === this.no?.id) {
      this.destroy();
      this.#onNo?.();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
