// `WndPetFoodMill` (`CWndPetFoodMill`, `src:_Interface/WndPetSys.cpp:1636-1820`, ref/22 §17 M, Plan/42 §19.4 M, 45 §18.7; WP-UI-33): `APP_PET_FOODMILL` (208 x 176,
// "Feed machine"), the pet food grinder of the NPC (`MMI_PET_FOODMILL`) and of the feed-maker item.
//   * `WIDC_STATIC1` carries the description, `WIDC_STATIC2` is the 32 x 32 slot; the ground item is shown with its count at the bottom right (the original
//     prints it 36 px right and 48 px below the slot's corner, `TID_GAME_KWAIBAWIBO_PRESENT_NUM`, blue);
//   * a bag item dropped on the slot asks the count (`CWndFoodConfirm(2)`, the request opens it); the count reserves that many of the stack; a drop that comes
//     from a pocket is refused with a message box (`TID_GAME_ERROR_FOOD_MILL_POCKET`);
//   * a double click on the slot gives the item back;
//   * OK sends `MakePetFeed` for an item that may be ground, else `TID_GAME_NOTFOOD`; the answer `PetMill` comes to `receiveResult`: no room (0) prints
//     `TID_GAME_LACKSPACE`, success (1) prints the count made and empties the slot; the tool window ends after any answer (`m_dwObjId != NULL_ID`);
//   * Cancel and the close button end the window; the slot is released from `onDestroy`.

import {
  APP_PET_FOODMILL,
  APP_INVENTORY,
  APP_BAG_EX,
  WIDC_CANCEL,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_STATIC2,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import { TextLayer } from '../../controls/TextLayer';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import { FONT, lineHeight, textWidth } from '../../text/fonts';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DialogItem } from '../dialogs/ports';
import type { FoodMillRequest } from './ports';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_SPRITES = 0x7a40;
const ID_TEXTS = 0x7a41;
/** `0xff0000ff`: the count under the slot. */
export const MILL_COUNT_COLOR = 0xff0000ff;

export interface FoodMillDeps {
  readonly request: FoodMillRequest;
  readonly tooltip: TooltipService;
}

export class WndPetFoodMill extends WndNeuz {
  readonly appId = APP_PET_FOODMILL;
  readonly #request: FoodMillRequest;
  readonly #tooltip: TooltipService;
  #slot: Rect = NO_RECT;
  #item: DialogItem | null = null;
  #count = 0;
  #sprites: SpriteLayer | null = null;
  #texts: TextLayer | null = null;
  #released = false;

  constructor(deps: FoodMillDeps) {
    super();
    this.#request = deps.request;
    this.#tooltip = deps.tooltip;
    this.putRegInfo = false;
  }

  /** `m_dwObjId`: the feed-maker item that opened the window, `null` = the NPC. */
  get tool(): number | null {
    return this.#request.tool;
  }
  /** The ground item in the slot and the count chosen for it. */
  get item(): DialogItem | null {
    return this.#item;
  }
  get count(): number {
    return this.#count;
  }
  get slotRect(): Rect {
    return this.#slot;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<Static>(WIDC_STATIC1)?.setTitle(this.#request.description);
    this.#slot = this.getWndCtrl(WIDC_STATIC2)?.rect ?? NO_RECT;
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.moveParentCenter();
  }

  // ---- the slot ----------------------------------------------------------------------------------------------------------------

  /** `SetItemForFeed( item, count )`: the item sits in the slot with its count reserved. */
  setItemForFeed(objId: number, count: number): boolean {
    const item = this.#request.item(objId);
    if (item === null) return false;
    this.#giveBack();
    this.#item = item;
    this.#count = count;
    this.#request.reserve(objId, count);
    this.#paint();
    return true;
  }

  /** Empties the slot (`SetExtra( 0 )` unless the item is being traded, which the request knows). */
  #giveBack(): void {
    if (this.#item) this.#request.reserve(this.#item.objId, 0);
    this.#item = null;
    this.#count = 0;
  }

  clearItem(): void {
    this.#giveBack();
    this.#paint();
  }

  #paint(): void {
    const it = this.#item;
    if (!it) {
      this.#sprites?.setEntries([]);
      this.#texts?.setItems([]);
      return;
    }
    const s = this.#slot;
    this.#sprites?.setEntries(
      it.icon === null ? [] : [{ key: 'item', path: it.icon, x: s.l, y: s.t, w: 32, h: 32 }],
    );
    const text = this.#request.countText(this.#count);
    this.#texts?.setItems([
      {
        x: s.l + 36 - Math.round(textWidth(FONT.text, text)),
        y: s.t + 48 - lineHeight(FONT.text),
        text,
        color: MILL_COUNT_COLOR,
      },
    ]);
  }

  // ---- input -------------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: a bag item on the slot asks its count; a pocket item is refused. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    const from = sc.from?.getFrameWnd()?.id ?? 0;
    if (from === APP_BAG_EX) {
      this.#request.pocketRefused();
      return false;
    }
    if (sc.kind !== SC.ITEM || from !== APP_INVENTORY) {
      this.env.wm.drag.forbid();
      return false;
    }
    const pt = {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
    if (!rectContains(this.#slot, pt)) return true;
    this.#giveBack();
    this.#paint();
    this.#request.askCount(sc.id, (count) => {
      this.setItemForFeed(sc.id, count);
    });
    return true;
  }

  override onLButtonDblClk(pt: Point): void {
    if (this.#item !== null && rectContains(this.#slot, pt)) this.clearItem();
  }

  override onMouseMove(pt: Point): void {
    if (this.#item === null || !rectContains(this.#slot, pt) || this.env.wm.isDraggingShortcut())
      return;
    const s = this.getScreenClientRect();
    this.#tooltip.putItem(
      this,
      this.#item,
      {
        l: this.#slot.l + s.l,
        t: this.#slot.t + s.t,
        r: this.#slot.r + s.l,
        b: this.#slot.b + s.t,
      },
      APP_PET_FOODMILL,
    );
  }

  /** OK: the item must be a gem that is not charged and costs something, else `TID_GAME_NOTFOOD`. */
  ok(): void {
    const it = this.#item;
    if (it === null) return;
    if (this.#request.accept(it.objId))
      this.#request.send(it.objId, this.#count, this.#request.tool);
  }

  /** `ReceiveResult( nResult, nCount )` of `PetMill`, then the tool window ends (`OnPetFoodMill`). */
  receiveResult(result: number, count: number): void {
    this.#request.result(result, count);
    if (result === 1) {
      // `if( !IsTrading( item ) ) SetExtra( 0 )`
      this.#giveBack();
      this.#paint();
    }
    if (this.#request.tool !== null) this.destroy();
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
    if (!this.#released) {
      this.#released = true;
      this.#giveBack();
    }
    this.#request.closed?.();
    super.onDestroy();
  }
}
