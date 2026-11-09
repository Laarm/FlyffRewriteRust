// `WndPetTransEggs` (`CWndPetTransEggs`, `src:_Interface/WndPetSys.cpp:1917-2159`, ref/22 §17 M, Plan/42 §19.4 M, 45 §18.7; WP-UI-33): `APP_PET_TRANS_EGGS` (240 x 320, "Egg
// Change"), the egg transformer: ten 32 x 32 slots (`WIDC_STATIC1..10`, two rows of five), the description (`WIDC_TEXT1`), OK and Cancel.
//   * a bag egg dropped on a slot (or handed over by the inventory's double click, `setItem`, which takes the first free slot) fills it; the request decides what
//     an egg is and prints the refusals (`TID_GAME_TRANS_EGGS_ERROR1` for a non-egg, `_ERROR2` for the summoned one); a stack in use (`extra > 0`) is refused;
//   * OK is on only when all ten slots are full; it sends the ten eggs (`TransformItem{0, 10 x {obj, 1}}`) and ends the window;
//   * a double click on a slot gives the egg back and turns OK off; a filled slot has an item tooltip; every egg is released when the window ends.

import {
  APP_INVENTORY,
  APP_PET_TRANS_EGGS,
  WIDC_CANCEL,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_STATIC10,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC7,
  WIDC_STATIC8,
  WIDC_STATIC9,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DialogItem } from '../dialogs/ports';
import type { TransEggsRequest } from './ports';

/** `MAX_TRANS_EGG`. */
export const MAX_TRANS_EGG = 10;
const SLOT_IDS = [
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC7,
  WIDC_STATIC8,
  WIDC_STATIC9,
  WIDC_STATIC10,
] as const;
const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_SPRITES = 0x7a60;

export interface TransEggsDeps {
  readonly request: TransEggsRequest;
  readonly tooltip: TooltipService;
}

export class WndPetTransEggs extends WndNeuz {
  readonly appId = APP_PET_TRANS_EGGS;
  readonly #request: TransEggsRequest;
  readonly #tooltip: TooltipService;
  readonly #eggs: (DialogItem | null)[] = Array.from({ length: MAX_TRANS_EGG }, () => null);
  #slots: Rect[] = Array.from({ length: MAX_TRANS_EGG }, () => NO_RECT);
  #sprites: SpriteLayer | null = null;

  constructor(deps: TransEggsDeps) {
    super();
    this.#request = deps.request;
    this.#tooltip = deps.tooltip;
    this.putRegInfo = false;
  }

  /** The eggs in the slots (`null` = empty), in slot order. */
  get eggs(): readonly (DialogItem | null)[] {
    return this.#eggs;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get slotRects(): readonly Rect[] {
    return this.#slots;
  }
  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  get full(): boolean {
    return this.#eggs.every((e) => e !== null);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.okButton?.enableWindow(false);
    this.getDlgItem<TextCtrl>(WIDC_TEXT1)?.setString(this.#request.description);
    this.#slots = SLOT_IDS.map((id) => this.getWndCtrl(id)?.rect ?? NO_RECT);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, this.getClientRect(false));
    this.moveParentCenter();
  }

  // ---- the slots ---------------------------------------------------------------------------------------------------------------

  #put(slot: number, objId: number): boolean {
    if (this.#eggs[slot] !== null) return false;
    const egg = this.#request.accept(objId);
    if (egg === null) return false;
    this.#eggs[slot] = egg;
    // `SetExtra( GetExtra() + 1 )`
    this.#request.reserve(egg.objId, 1);
    this.#refresh();
    return true;
  }

  /** `SetItem( item )` (the inventory's double click): the first empty slot. */
  setItem(objId: number): boolean {
    const slot = this.#eggs.findIndex((e) => e === null);
    return slot >= 0 && this.#put(slot, objId);
  }

  clearSlot(slot: number): void {
    const egg = this.#eggs[slot];
    if (!egg) return;
    this.#request.reserve(egg.objId, 0);
    this.#eggs[slot] = null;
    this.#refresh();
  }

  #refresh(): void {
    this.okButton?.enableWindow(this.full);
    this.#sprites?.setEntries(
      this.#eggs.flatMap((egg, i) =>
        egg && this.#request.eggIcon
          ? [
              {
                key: `egg${String(i)}`,
                path: this.#request.eggIcon,
                x: this.#slots[i]?.l ?? 0,
                y: this.#slots[i]?.t ?? 0,
                w: 32,
                h: 32,
              },
            ]
          : [],
      ),
    );
  }

  // ---- input -------------------------------------------------------------------------------------------------------------------

  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM || (sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) {
      this.env.wm.drag.forbid();
      return false;
    }
    const pt = {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
    const slot = this.#slots.findIndex((r) => rectContains(r, pt));
    if (slot >= 0) this.#put(slot, sc.id);
    return true;
  }

  override onLButtonDblClk(pt: Point): void {
    const slot = this.#slots.findIndex((r) => rectContains(r, pt));
    if (slot >= 0) this.clearSlot(slot);
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const slot = this.#slots.findIndex((r) => rectContains(r, pt));
    const egg = slot >= 0 ? this.#eggs[slot] : null;
    const rect = slot >= 0 ? this.#slots[slot] : undefined;
    if (!egg || !rect) return;
    const s = this.getScreenClientRect();
    this.#tooltip.putItem(this, egg, rectOffset(rect, s.l, s.t), APP_PET_TRANS_EGGS);
  }

  /** OK: all ten eggs go to the server. */
  ok(): void {
    if (!this.full) return;
    this.#request.send(this.#eggs.flatMap((e) => (e ? [e.objId] : [])));
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
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
    for (const egg of this.#eggs) if (egg) this.#request.reserve(egg.objId, 0);
    this.#request.closed?.();
    super.onDestroy();
  }
}
