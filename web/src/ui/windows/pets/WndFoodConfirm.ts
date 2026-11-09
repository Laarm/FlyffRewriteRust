// `WndFoodConfirm` (`CWndFoodConfirm`, `src:_Interface/WndPetSys.cpp:1055-1263`, ref/22 §17 M, Plan/42 §19.4 M; WP-UI-33): `APP_PET_ITEM` (272 x 160), "how many?" with a
// quantity edit. It starts at the whole stack (at most 1000), accepts digits only and is clamped to the stack and to 1000; OK (the default button) and Enter take
// 1..1000 and hand the count to the request, then the window ends; Cancel and the close button end it. The question and the title differ by the parent: the pet
// food (`nParent 1`, `TID_GAME_PETFOOD_*`) and the food mill (`nParent 2`, `TID_GAME_ITEMCOUNT_*`).

import {
  APP_PET_ITEM,
  WIDC_CANCEL,
  WIDC_EDIT1,
  WIDC_OK,
  WIDC_STATIC1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { FoodConfirmRequest } from './ports';

/** The most the box takes (`DropNum <= 1000`, the "0723" fix of the original). */
export const FOOD_MAX = 1000;

export class WndFoodConfirm extends WndNeuz {
  readonly appId = APP_PET_ITEM;
  readonly #request: FoodConfirmRequest;
  #edit: EditCtrl | null = null;

  constructor(request: FoodConfirmRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get quantityEdit(): EditCtrl | null {
    return this.#edit;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.okButton?.setDefault(true);
    this.#edit?.addWndStyle(EBS_NUMBER);
    this.#edit?.setFocus();
    const count = this.#request.count;
    if (count < 1) {
      // `m_pItemElem == NULL`: the window ends at once
      this.destroy();
      return;
    }
    this.#edit?.setString(String(count === 1 ? 1 : Math.min(count, FOOD_MAX)));
    this.getDlgItem<Static>(WIDC_STATIC1)?.setTitle(this.#request.question);
    this.setTitle(this.#request.title);
    this.moveParentCenter();
  }

  /** `OnDraw`: a single item always reads 1; a number above 1000 reads 1000, above the stack the stack. */
  #clamp(): void {
    const edit = this.#edit;
    if (!edit) return;
    const count = this.#request.count;
    if (count === 1) {
      if (edit.getString() !== '1') edit.setString('1');
      return;
    }
    const n = Number.parseInt(edit.getString(), 10);
    if (!Number.isFinite(n)) return;
    if (n > FOOD_MAX) edit.setString(String(FOOD_MAX));
    else if (n > count) edit.setString(String(count));
  }

  override process(now: number): void {
    this.#clamp();
    super.process(now);
  }

  /** The count OK would send (`0` = nothing). */
  get quantity(): number {
    const n = Number.parseInt(this.#edit?.getString() ?? '', 10);
    return Number.isFinite(n) ? n : 0;
  }

  /** OK or Enter: `DropNum != 0 && DropNum <= 1000` goes to the request; the window ends either way. */
  ok(): void {
    this.#clamp();
    const n = this.quantity;
    if (this.#request.count >= 1 && n !== 0 && n <= FOOD_MAX) this.#request.confirm(n);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if ((msg === Wnm.CLICKED && id === WIDC_OK) || msg === Wnm.EN_RETURN) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
