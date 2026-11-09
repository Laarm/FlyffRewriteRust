// Shared pieces of the beauty shop windows. `OnInitialUpdate` (`WndShop.cpp:1936-1950`, `3215-3230`) moves the window next to the inventory
// window, on the side that has more room (`ptInventory.x > width / 2` puts it to the left), at the inventory's top; without an inventory it
// stays centred. The draft model of both shops turns on a turntable.

import { APP_INVENTORY } from '../../../gen/defs/resdata';
import type { WndNeuz } from '../../wnd/WndNeuz';

export function placeBesideInventory(w: WndNeuz): void {
  const inv = w.env.wm.get(APP_INVENTORY);
  if (!inv) {
    w.moveParentCenter();
    return;
  }
  const r = inv.rectWindow;
  const width = w.rectWindow.r - w.rectWindow.l;
  const x = r.l > width / 2 ? r.l - width : r.r;
  w.move({ x, y: r.t });
}

/** The turntable of the draft model: `RotationY( g_tmCurrent / 1000 )` radians, as degrees. */
export const turntableDegrees = (nowMs: number): number => (((nowMs / 1000) * 180) / Math.PI) % 360;
