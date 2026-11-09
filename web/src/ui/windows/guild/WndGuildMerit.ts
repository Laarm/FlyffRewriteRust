// `WndGuildMerit` (`CWndGuildMerit` with `CWndGuildMeritCtrl`, `src:_Interface/WndGuildMerit.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-47): `APP_GUILD_MERIT1` (272 x 224), the
// contribution of a member: the penya (`WIDC_EDIT1`, digits, starts at "0", focused) and the gems of the bag (`WIDC_CHECK2` lists every gem stack of the bag in a `MeritList` over
// the rectangle of `WIDC_LISTBOX1`, six per row, and reserves it with `SetExtra`; unchecking releases them). `WIDC_BUTTON1` (also Enter in the edit) asks the caller to contribute
// the penya typed and, when the box is checked, the gems; `WIDC_BUTTON2` cancels. The reservations go back on every way out of the window. The caller (the bank window's support
// button opens it) runs the pre-checks (`CanContribute`) and sends `GuildContributionReq`; the success line comes with the server's answer (FIX: the original printed it on request).

import {
  APP_GUILD_MERIT1,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_CHECK2,
  WIDC_EDIT1,
  WIDC_LISTBOX1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { EBS_NUMBER, Wnm } from '../../layout/styles';
import { MeritList } from '../../lists/MeritList';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { InventoryItem } from '../char/ports';
import { atoiPenya } from './penya';
import type { MeritRequest } from './ports';

const ID_LIST = 100;

export interface MeritDeps {
  readonly tooltip: TooltipService;
}

export class WndGuildMerit extends WndNeuz {
  readonly appId = APP_GUILD_MERIT1;
  readonly #request: MeritRequest;
  readonly #deps: MeritDeps;
  #edit: EditCtrl | null = null;
  #list: MeritList | null = null;
  /** The stacks listed (and reserved) while the box is checked. */
  #gems: readonly InventoryItem[] = [];
  #checked = false;

  constructor(request: MeritRequest, deps: MeritDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }
  get list(): MeritList | null {
    return this.#list;
  }
  get gemCheck(): Button | null {
    return this.getDlgItem<Button>(WIDC_CHECK2);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  /** The stacks listed now. */
  get gems(): readonly InventoryItem[] {
    return this.#gems;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.addWndStyle(EBS_NUMBER);
    this.#edit?.setString('0');
    const at = this.getWndCtrl(WIDC_LISTBOX1)?.rect;
    if (at) {
      this.#list = this.addControl(
        new MeritList({
          hover: itemHover({
            tooltip: this.#deps.tooltip,
            owner: this,
            flag: APP_GUILD_MERIT1,
            itemAt: (slot) => this.#gems[slot] ?? null,
          }),
        }),
        ID_LIST,
        at,
      );
    }
    this.moveParentCenter();
    this.#edit?.setFocus();
  }

  #cell(item: InventoryItem): IconCellData {
    return {
      icon: item.icon,
      count: item.count,
      showCount: item.stackable,
      dim: false,
      tint: item.expired ? 'red' : null,
      cooldown: null,
    };
  }

  /** `WIDC_CHECK2`: lists the gems and reserves them, or releases them. */
  #toggleGems(on: boolean): void {
    if (on === this.#checked) return;
    this.#checked = on;
    if (on) {
      this.#gems = this.#request.gems();
      for (const g of this.#gems) this.#request.reserve(g.objId, g.count);
      this.#list?.setCells(this.#gems.map((g) => this.#cell(g)));
    } else {
      this.#release();
      this.#list?.setCells([]);
    }
  }

  #release(): void {
    for (const g of this.#gems) this.#request.reserve(g.objId, 0);
    this.#gems = [];
  }

  /** `WIDC_BUTTON1` / Enter: contribute and close. */
  ok(): void {
    const penya = atoiPenya(this.#edit?.getString() ?? '');
    this.#request.submit(penya, this.#checked);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_RETURN) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_CHECK2:
          this.#toggleGems(this.gemCheck?.getCheck() !== 0);
          return true;
        case WIDC_BUTTON1:
          this.ok();
          return true;
        case WIDC_BUTTON2:
          this.destroy();
          return true;
        default:
          break;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#deps.tooltip.cancel();
    this.#release();
    this.#request.closed?.();
    super.onDestroy();
  }
}
