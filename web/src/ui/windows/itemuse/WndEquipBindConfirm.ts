// `WndEquipBindConfirm` (`CWndEquipBindConfirm`, `src:_Interface/WndField.cpp:25899-26010`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4;
// WP-UI-28): `APP_EQUIP_BIND_CONFIRM` (336 x 160, modal, `WBS_KEY`). It asks before an `IP_FLAG_EQUIP_BIND` item that is not bound
// yet is worn: the text `TID_TOOLTIP_EQUIPBIND_CONFIRM` and Yes / No. The hidden focus edit (moved to (-100, -100)) takes the Enter
// key: Enter is Yes. The decision behind Yes (`SendDoUseItem` for the double click, `SendDoEquip` for the drag) is the caller's.

import {
  APP_EQUIP_BIND_CONFIRM,
  WIDC_EDIT_FOCUS,
  WIDC_EQUIP_BIND_NO,
  WIDC_EQUIP_BIND_TEXT,
  WIDC_EQUIP_BIND_YES,
} from '../../../gen/defs/resdata';
import { TID_TOOLTIP_EQUIPBIND_CONFIRM } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_KEY, WBS_MODAL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { EquipBindConfirmRequest } from './ports';

export interface EquipBindDeps {
  readonly request: EquipBindConfirmRequest;
  readonly getText: (tid: number) => string;
}

export class WndEquipBindConfirm extends WndNeuz {
  readonly appId = APP_EQUIP_BIND_CONFIRM;
  readonly #deps: EquipBindDeps;
  #text: TextCtrl | null = null;
  #edit: EditCtrl | null = null;

  constructor(deps: EquipBindDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_EQUIP_BIND_CONFIRM, WBS_MODAL | WBS_KEY)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
  }

  get text(): TextCtrl | null {
    return this.#text;
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_EQUIP_BIND_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_EQUIP_BIND_NO);
  }
  get focusEdit(): EditCtrl | null {
    return this.#edit;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_EQUIP_BIND_TEXT);
    this.#text?.setString(this.#deps.getText(TID_TOOLTIP_EQUIPBIND_CONFIRM));
    this.#text?.enableWindow(false);
    // the hidden edit takes the keyboard (Enter = Yes)
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT_FOCUS);
    this.#edit?.move({ x: -100, y: -100 });
    this.#edit?.setFocus();
    this.yesButton?.setDefault(true);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (
      (msg === Wnm.EN_RETURN && id === WIDC_EDIT_FOCUS) ||
      (msg === Wnm.CLICKED && id === WIDC_EQUIP_BIND_YES)
    ) {
      this.#deps.request.yes();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_EQUIP_BIND_NO) {
      this.#deps.request.no?.();
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
