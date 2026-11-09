// `CWndGold` over `WIDC_BANKGO_0..2` of the bank window (WndField.cpp:499-518, `m_wndGold[i].Create("g", WBS_NODRAWFRAME, rect, this, ..)`): the
// invisible button over the coin of a compartment. Pressing it picks the penya of that compartment up: a `SHORTCUT_ITEM` with `data = 0`
// and the gold-seed icon, dragged to the inventory (the withdrawal) or to another compartment. The original found the compartment through
// the focused child of the bank window; the payload carries it in `userId`. A disabled compartment (`EnableWindow(FALSE)`) takes no press.

import { ITYPE_ITEM } from '../../../gen/defs/define';
import { viewOf } from '../../controls/bind';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { WTYPE_CUSTOM } from '../../layout/styles';
import { Control } from '../../wnd/Control';

/** `itm_GolGolSeed.dds`. */
export const BANK_GOLD_ICON = 'item/itm_golgolseed.dds';

function BankGoldIconView() {
  return <div className="bank-gold-icon" />;
}

export class BankGoldIcon extends Control {
  /** The compartment (0..2) this coin belongs to. */
  readonly slot: number;

  readonly #onDrop: ((sc: Shortcut) => boolean) | undefined;

  constructor(slot: number, onDrop?: (sc: Shortcut) => boolean) {
    super(WTYPE_CUSTOM, null);
    this.slot = slot;
    this.#onDrop = onDrop;
    this.view = viewOf<BankGoldIcon>(BankGoldIconView);
  }

  /** The payload of the drag: `dwData == 0` marks the penya, `userId` the compartment it leaves. */
  payload(): Shortcut {
    return {
      kind: SC.ITEM,
      id: this.slot,
      type: ITYPE_ITEM,
      index: 0,
      userId: this.slot,
      data: 0,
      icon: BANK_GOLD_ICON,
      from: this,
    };
  }

  /** FIX: the coin of an inventory (or of another compartment) dropped on the coin icon goes into this compartment. */
  override onDropIcon(sc: Shortcut): boolean {
    return this.#onDrop?.(sc) ?? false;
  }

  override onLButtonDown(): void {
    if (this.env.wm.isDraggingShortcut()) return;
    this.env.wm.drag.arm(this, () => this.payload());
  }
}
