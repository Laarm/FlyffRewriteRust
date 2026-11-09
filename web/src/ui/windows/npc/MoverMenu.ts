// `MoverMenu` (`CWndWorld::m_wndMenuMover`, `CWndMenu` of the world window; Plan/45 §11.1; WP-CFEAT-18): the popup that lists
// the menu actions of a player or an NPC at the top-right corner of its bound rect. A `Menu` whose commands do not go to an
// owner window but to a callback: the feature that opened it decides what an id means.

import { type Point, rectHeight, rectWidth } from '../../layout/geometry';
import { Menu } from '../../controls/Menu';
import type { WndBase } from '../../wnd/WndBase';

export interface MoverMenuItem {
  /** The `MMI_*` id (the command id of the item). */
  readonly id: number;
  readonly text: string;
}

export class MoverMenu extends Menu {
  /** The picked item (called once per click). */
  onPick: ((id: number) => void) | null = null;

  /** Creates the (hidden) popup under `root`. */
  attach(root: WndBase): boolean {
    return this.createMenu(root);
  }

  /** `DeleteAllMenu` + `AppendMenu` per item, moved to `at` and focused; the popup stays inside the layout rect. */
  show(items: readonly MoverMenuItem[], at: Point): void {
    this.deleteAllMenu();
    for (const it of items) this.appendMenu(it.text, it.id);
    const layout = this.env.wm.layoutRect;
    const w = rectWidth(this.rectWindow);
    const h = rectHeight(this.rectWindow);
    const x = Math.max(layout.l, Math.min(at.x, layout.r - w));
    const y = Math.max(layout.t, Math.min(at.y, layout.b - h));
    this.move({ x, y });
    this.setVisible(true);
    this.setFocus();
  }

  hide(): void {
    this.setVisibleAllMenu(false);
  }

  get shown(): boolean {
    return this.isVisible();
  }

  /** The command of an item goes to the callback, then the popup closes. */
  override onCommand(id: number): boolean {
    this.setVisibleAllMenu(false);
    this.onPick?.(id);
    return true;
  }

  /** The click notification is the same click as the command: ignored (the command does the work). */
  override onChildNotify(): boolean {
    return true;
  }
}
