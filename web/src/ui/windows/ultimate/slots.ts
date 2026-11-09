// Helpers the ultimate windows share (WP-UI-32): the window-to-client conversion of a drop point, the "dragged from the bag" test and the
// strip entries of an item slot.

import { APP_INVENTORY } from '../../../gen/defs/resdata';
import type { StripEntry } from '../../controls/IconStrip';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { Point, Rect } from '../../layout/geometry';
import type { WndNeuz } from '../../wnd/WndNeuz';

/** The edge of an item slot and of its picture. */
export const SLOT_SIZE = 32;

export const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };

/** A point in the WINDOW (frame) coordinates of a drop as a point in the CLIENT coordinates the layout rects use. */
export function toClient(w: WndNeuz, ptWindow: Point): Point {
  return {
    x: ptWindow.x - (w.rectClient.l - w.rectWindow.l),
    y: ptWindow.y - (w.rectClient.t - w.rectWindow.t),
  };
}

/** An item dragged out of the bag (`g_pPlayer->GetItemId( pShortcut->m_dwId )` only finds bag items). */
export function fromBag(sc: Shortcut): boolean {
  return sc.kind === SC.ITEM && (sc.from?.getFrameWnd()?.id ?? 0) === APP_INVENTORY;
}

/** The picture of an item in a slot (`pTexture->Render( p2DRender, CPoint( rect.left, rect.top ) )`), `alpha` 0..255. */
export function slotEntry(key: string, icon: string | null, rect: Rect, alpha = 255): StripEntry {
  return { key, icon, x: rect.l, y: rect.t, w: SLOT_SIZE, h: SLOT_SIZE, alpha };
}
