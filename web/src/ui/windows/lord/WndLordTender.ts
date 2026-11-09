// `WndLordTender` (`CWndLordTender`, `src:_Interface/WndLord.cpp:318-454`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-54): `APP_LORD_TENDER` (718, 304 x 576, "Bid"), the bid for the lord candidacy.
// `TEXT1` shows the information panel `lordcandidate` of the help bundle, `TEXT2` the candidates in the order of the list ("  %d            %s \n", one blank less from rank 10; a name that is not answered
// yet prints as nothing, the "...." branch of the original is dead code), `EDIT1` the amount: "0" at first, then the own total (`RefreshDeposit`, also called after every deposit message). The edit takes
// digits only, at most 15 (FIX: the original `atol` wrapped into a DWORD). `BUTTON1` sends `ElectionAddDepositReq{total}` for 100 000 000 or more and opens the box `TID_UPGRADE_ERROR_NOMONEY` for
// less; `BUTTON2` closes. (45 §28.2 asks for the gold with commas: the 718 layout has no control for it, the edit keeps the raw digits `%I64d`.)

import {
  APP_LORD_TENDER,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_EDIT1,
  WIDC_TEXT1,
  WIDC_TEXT2,
} from '../../../gen/defs/resdata';
import { TID_UPGRADE_ERROR_NOMONEY } from '../../../gen/defs/text';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { MAX_DEPOSIT_DIGITS, MIN_DEPOSIT, type LordView, type LordWindowDeps } from './ports';
import { parseDeposit, tenderListText } from './rows';

export class WndLordTender extends WndNeuz {
  readonly appId = APP_LORD_TENDER;
  readonly #deps: LordWindowDeps;
  #view: LordView | null = null;
  #shown = '';

  constructor(deps: LordWindowDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get infoText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get listText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT2);
  }
  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const text = this.#deps.ports.panel('lordcandidate');
    if (text !== '') this.infoText?.setString(text);
    const edit = this.edit;
    if (edit) {
      edit.addWndStyle(EBS_NUMBER);
      edit.setMaxLength(MAX_DEPOSIT_DIGITS);
      edit.setString('0');
    }
    this.refreshDeposit();
    this.moveParentCenter();
    this.#apply(this.#deps.ports.view.get());
  }

  /** `RefreshDeposit`: the edit shows the own total when the own character is a candidate. */
  refreshDeposit(): void {
    const view = this.#deps.ports.view.get();
    const own = view.candidates.find((c) => c.id === view.selfId);
    if (own) this.edit?.setString(String(own.deposit));
  }

  /** `OnDraw`: the list text of every candidate with a player id. */
  #apply(view: LordView): void {
    this.#view = view;
    const text = tenderListText(view.candidates.filter((c) => c.id > 0));
    if (text === this.#shown) return;
    this.#shown = text;
    this.listText?.setString(text);
  }

  override process(now: number): void {
    const view = this.#deps.ports.view.get();
    if (view !== this.#view) this.#apply(view);
    super.process(now);
  }

  /** `BUTTON1`: sends the bid; returns the amount sent, `null` for the refusal. */
  bid(): number | null {
    const cost = parseDeposit(this.edit?.getString() ?? '');
    if (cost >= MIN_DEPOSIT) {
      this.#deps.ports.actions.addDeposit(cost);
      return cost;
    }
    this.#deps.host.box(this.#deps.host.text(TID_UPGRADE_ERROR_NOMONEY));
    return null;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.bid();
        return true;
      }
      if (id === WIDC_BUTTON2) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
