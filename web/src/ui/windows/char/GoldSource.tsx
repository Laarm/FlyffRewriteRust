// `CWndGold` (WndField.cpp:499-525; WP-UI-21): the invisible button over the gold coin of the inventory (and the bank, the
// guild bank). Pressing it picks up the penya: a `SHORTCUT_ITEM` with `data = 0` and the gold-seed icon, dragged to the
// trade, bank or vendor windows. Everything else about the coin is in the window's background picture.

import { ITYPE_ITEM } from '../../../gen/defs/define';
import { viewOf } from '../../controls/bind';
import { WTYPE_CUSTOM } from '../../layout/styles';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { Control } from '../../wnd/Control';

/** `itm_GolGolSeed.dds`. */
export const GOLD_ICON = 'item/itm_golgolseed.dds';

function GoldSourceView() {
  return <div className="gold-source" />;
}

export class GoldSource extends Control {
  constructor() {
    super(WTYPE_CUSTOM, null);
    this.view = viewOf<GoldSource>(GoldSourceView);
  }

  /** The payload of the drag: `dwData == 0` marks the gold. */
  payload(): Shortcut {
    return {
      kind: SC.ITEM,
      id: 0,
      type: ITYPE_ITEM,
      index: 0,
      userId: 0,
      data: 0,
      icon: GOLD_ICON,
      from: this,
    };
  }

  override onLButtonDown(): void {
    if (this.env.wm.isDraggingShortcut()) return;
    this.env.wm.drag.arm(this, () => this.payload());
  }
}
