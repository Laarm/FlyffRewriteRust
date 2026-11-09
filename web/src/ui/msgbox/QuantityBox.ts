// The quantity box (`CWndTradeGold` on `APP_TRADE_GOLD`, WndField.cpp:8787-9000; Plan/42 §14): "Move Items" with a number
// edit that starts at the maximum. OK (or Enter in the edit) accepts the value clamped to `[1, max]`; the edit takes
// digits only (a letter never gets in); Cancel and the close button drop the box. The window is modal.

import { APP_TRADE_GOLD } from '../../gen/defs/resdata';
import type { Button } from '../controls/Button';
import type { EditCtrl } from '../controls/EditCtrl';
import type { Static } from '../controls/Static';
import { EBS_NUMBER, WBS_MODAL, Wnm } from '../layout/styles';
import type { Shortcut } from '../dnd/shortcut';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';

/** The value an OK produces: the digits typed, at least 1, at most `max` (`nCost < 1 -> 1`, `nCost > have -> have`). */
export function clampQuantity(text: string, max: number): number {
  const n = Number.parseInt(text, 10);
  const v = Number.isFinite(n) ? n : 0;
  return Math.max(1, Math.min(v, Math.max(1, max)));
}

/**
 * The two captions that the callers of the original set on the layout statics (`pStatic->m_strTitle = strMain` on `WIDC_STATIC`, the question,
 * and `pStaticCount->m_strTitle = strCount` on `WIDC_CONTROL1`, the label before the edit): "Move how many items?" / "Number:", "How many do
 * you want to move?" / "Penya:". Without them the layout's own texts show.
 */
export interface QuantityLabels {
  readonly prompt?: string;
  readonly label?: string;
}

export class QuantityBoxWnd extends WndNeuz {
  readonly appId = APP_TRADE_GOLD;
  readonly max: number;
  readonly #onOk: (n: number) => void;
  readonly #onCancel: (() => void) | undefined;
  readonly #boxTitle: string | undefined;
  readonly #labels: QuantityLabels | undefined;
  edit: EditCtrl | null = null;

  constructor(
    max: number,
    onOk: (n: number) => void,
    title?: string,
    onCancel?: () => void,
    labels?: QuantityLabels,
  ) {
    super();
    this.max = Math.max(1, Math.floor(max));
    this.#onOk = onOk;
    this.#onCancel = onCancel;
    this.#boxTitle = title;
    this.#labels = labels;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    if (this.#boxTitle !== undefined) this.setTitle(this.#boxTitle);
    if (this.#labels?.prompt !== undefined)
      this.getDlgItem<Static>('WIDC_STATIC')?.setTitle(this.#labels.prompt);
    if (this.#labels?.label !== undefined)
      this.getDlgItem<Static>('WIDC_CONTROL1')?.setTitle(this.#labels.label);
    const edit = this.getDlgItem<EditCtrl>('WIDC_EDIT');
    const ok = this.getDlgItem<Button>('WIDC_OK');
    ok?.setDefault(true);
    if (edit) {
      edit.addWndStyle(EBS_NUMBER); // digits only
      edit.setString(String(this.max)); // the initial value is the maximum
      edit.setFocus();
      this.edit = edit;
    }
    this.moveParentCenter();
  }

  /** OK or Enter in the edit accepts; Cancel and the title-bar close drop the box. */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const ok = this.getDlgItem('WIDC_OK');
    const cancel = this.getDlgItem('WIDC_CANCEL');
    if (msg === Wnm.EN_RETURN || (msg === Wnm.CLICKED && ok?.id === id)) {
      const n = clampQuantity(this.edit?.getString() ?? '', this.max);
      this.destroy();
      this.#onOk(n);
      return true;
    }
    if (msg === Wnm.CLICKED && cancel?.id === id) {
      this.destroy();
      this.#onCancel?.();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
