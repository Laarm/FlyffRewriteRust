// `WndRemoveJewel` (`CWndRemoveJewel`, `src:_Interface/WndField.cpp:22990-23420`, `__CSC_VER12_4`, ref/22 §17 L, Plan/42 §19.4 L, 45 §18.6; WP-UI-32 /
// WP-UI-33 / WP-CFEAT-26): `APP_SMELT_REMOVE_JEWEL` (272 x 432), "Remove Jewel from Ultimate Weapon". Two slots: the ultimate weapon
// (`WIDC_PIC_SLOT`: an ultimate weapon with at least one socket, else `TID_GAME_REMOVE_JEWEL_ERROR2`) and a moonstone (`WIDC_PIC_SLOT1`: the
// faint moonstone picture until one is there, anything else answers `TID_GAME_REMOVE_JEWEL_ERROR1`). Once the weapon is in, one row per socket
// shows the jewel's picture (`WIDC_JEWEL_SLOT1..5`) and its attribute in the added-option colour (`WIDC_JEWEL_SLOT_INFO1..5`).
// `features.improved_ulti_gems` hides the five old attributes: their rows keep the picture and lose the text (`jewelLabel` of the request).
// Both items are dropped or double-clicked in the bag (`feed`: a material takes the moonstone slot, a weapon the weapon slot); a double click on a
// slot gives the item back. Start (both slots filled) asks `APP_SMELT_REMOVE_CONFIRM` ("Confirm Jewel Removal", `TID_GAME_REMOVEJEWEL`); Yes sends
// `UltimateRemoveGem{weapon, moonstone}` and closes the window, No leaves everything as it was. The server removes the LAST filled socket.
//
// KEEP-DEVIATION: the original's Start sends at once; the confirmation is Plan/45 §18.6's (the dead `APP_SMELT_REMOVE_CONFIRM` of the old
// cursor path asked the same question).

import {
  APP_SMELT_REMOVE_CONFIRM,
  APP_SMELT_REMOVE_JEWEL,
  WIDC_JEWEL_SLOT1,
  WIDC_JEWEL_SLOT2,
  WIDC_JEWEL_SLOT3,
  WIDC_JEWEL_SLOT4,
  WIDC_JEWEL_SLOT5,
  WIDC_JEWEL_SLOT_INFO1,
  WIDC_JEWEL_SLOT_INFO2,
  WIDC_JEWEL_SLOT_INFO3,
  WIDC_JEWEL_SLOT_INFO4,
  WIDC_JEWEL_SLOT_INFO5,
  WIDC_NO,
  WIDC_PIC_SLOT,
  WIDC_PIC_SLOT1,
  WIDC_START,
  WIDC_TEXT1,
  WIDC_YES,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_REMOVE_JEWEL_ERROR1,
  TID_GAME_REMOVE_JEWEL_ERROR2,
  TID_GAME_TOOLTIP_REMOVEJEWEL1,
  TID_GAME_TOOLTIP_REMOVEJEWEL2,
  TID_GAME_TOOLTIP_REMOVEJEWEL3,
} from '../../../gen/defs/text';
import { IconStrip } from '../../controls/IconStrip';
import type { TextCtrl } from '../../controls/TextCtrl';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm, WTBID_CLOSE } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { JEWEL_ROWS } from './constants';
import type { RemoveJewelRequest, UltimateItem } from './ports';
import { NO_RECT, fromBag, slotEntry, toClient } from './slots';

const JEWEL_IDS = [
  WIDC_JEWEL_SLOT1,
  WIDC_JEWEL_SLOT2,
  WIDC_JEWEL_SLOT3,
  WIDC_JEWEL_SLOT4,
  WIDC_JEWEL_SLOT5,
] as const;
const INFO_IDS = [
  WIDC_JEWEL_SLOT_INFO1,
  WIDC_JEWEL_SLOT_INFO2,
  WIDC_JEWEL_SLOT_INFO3,
  WIDC_JEWEL_SLOT_INFO4,
  WIDC_JEWEL_SLOT_INFO5,
] as const;

export interface RemoveJewelDeps {
  readonly request: RemoveJewelRequest;
  readonly getText: (tid: number) => string;
  readonly tooltip: TooltipService;
}

export class WndRemoveJewel extends WndNeuz {
  readonly appId = APP_SMELT_REMOVE_JEWEL;
  readonly #deps: RemoveJewelDeps;
  #weapon: UltimateItem | null = null;
  #moonstone: UltimateItem | null = null;
  #weaponSlot: Rect = NO_RECT;
  #moonSlot: Rect = NO_RECT;
  #jewelSlots: Rect[] = [];
  #infoSlots: Rect[] = [];
  #strip: IconStrip | null = null;
  #texts: TextLayer | null = null;
  #description: TextCtrl | null = null;
  /** The confirmation is open. */
  #asking = false;

  constructor(deps: RemoveJewelDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get weapon(): UltimateItem | null {
    return this.#weapon;
  }
  get moonstone(): UltimateItem | null {
    return this.#moonstone;
  }
  get weaponSlot(): Rect {
    return this.#weaponSlot;
  }
  get moonstoneSlot(): Rect {
    return this.#moonSlot;
  }
  get strip(): IconStrip | null {
    return this.#strip;
  }
  get textLayer(): TextLayer | null {
    return this.#texts;
  }
  get descriptionText(): TextCtrl | null {
    return this.#description;
  }
  get startEnabled(): boolean {
    return this.getDlgItem(WIDC_START)?.isWindowEnabled() ?? false;
  }
  /** The confirmation of Start is open. */
  get asking(): boolean {
    return this.#asking;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rect = (id: number): Rect => this.getWndCtrl(id)?.rect ?? NO_RECT;
    this.#weaponSlot = rect(WIDC_PIC_SLOT);
    this.#moonSlot = rect(WIDC_PIC_SLOT1);
    this.#jewelSlots = JEWEL_IDS.map(rect);
    this.#infoSlots = INFO_IDS.map(rect);
    this.#description = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#description?.setString(this.#deps.request.description);
    const client = this.getClientRect(false);
    this.#strip = this.addControl(new IconStrip(), 0x7f51, client);
    this.#texts = this.addControl(new TextLayer(), 0x7f52, client);
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#paint();
    this.moveParentCenter();
  }

  // ---- the slots ----------------------------------------------------------------------------------------------------------------

  /** An ultimate weapon with at least one socket. */
  static isRemovable(item: UltimateItem): boolean {
    const w = item.weapon;
    return w !== null && w.tier === 'ultimate' && w.directOrMagic && w.sockets > 0;
  }

  /** `OnDropIcon`: the weapon onto its slot, a moonstone onto the other. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (!fromBag(sc)) return false;
    const pt = toClient(this, ptWindow);
    const item = this.#deps.request.item(sc.id);
    if (!item) {
      // a worn item or one the bag does not have: the forbid cursor
      this.env.wm.drag.forbid();
      return false;
    }
    if (rectContains(this.#weaponSlot, pt)) this.#takeWeapon(item);
    if (rectContains(this.#moonSlot, pt)) this.#takeMoonstone(item);
    this.#sync();
    return true;
  }

  /**
   * `SetItem` (the double click of a bag item, rule D3 `feed`): a material is taken for the moonstone while the slot is free (anything but a
   * moonstone prints `TID_GAME_REMOVE_JEWEL_ERROR1`), otherwise the item is the weapon (`TID_GAME_REMOVE_JEWEL_ERROR2` when it is none).
   */
  feed(objId: number): boolean {
    const item = this.#deps.request.item(objId);
    if (!item) return false;
    if (this.#moonstone === null && item.material) this.#takeMoonstone(item);
    else if (this.#weapon === null) this.#takeWeapon(item);
    this.#sync();
    return true;
  }

  #takeWeapon(item: UltimateItem): void {
    const req = this.#deps.request;
    if (this.#weapon !== null) return;
    if (!WndRemoveJewel.isRemovable(item)) {
      req.say(TID_GAME_REMOVE_JEWEL_ERROR2);
      return;
    }
    this.#weapon = item;
    req.setExtra(item.objId, item.extra + 1);
  }

  #takeMoonstone(item: UltimateItem): void {
    const req = this.#deps.request;
    if (this.#moonstone !== null) return;
    if (item.role !== 'moonstone') {
      req.say(TID_GAME_REMOVE_JEWEL_ERROR1);
      return;
    }
    this.#moonstone = item;
    req.setExtra(item.objId, item.extra + 1);
  }

  /** `OnLButtonDblClk`: the item of the slot under the pointer goes back. */
  override onLButtonDblClk(pt: Point): void {
    if (this.#asking) return;
    const req = this.#deps.request;
    if (rectContains(this.#weaponSlot, pt) && this.#weapon) {
      req.setExtra(this.#weapon.objId, 0);
      this.#weapon = null;
    }
    if (rectContains(this.#moonSlot, pt) && this.#moonstone) {
      req.setExtra(this.#moonstone.objId, 0);
      this.#moonstone = null;
    }
    this.#sync();
  }

  /** `OnMouseWndSurface`: the plain tooltips of the two slots and of the jewel rows. */
  override onMouseMove(pt: Point): void {
    const s = this.getScreenClientRect();
    const tip = (r: Rect, tid: number): void => {
      this.#deps.tooltip.put(this, this.#deps.getText(tid), rectOffset(r, s.l, s.t), 1);
    };
    if (rectContains(this.#weaponSlot, pt)) {
      tip(this.#weaponSlot, TID_GAME_TOOLTIP_REMOVEJEWEL1);
      return;
    }
    if (rectContains(this.#moonSlot, pt)) {
      tip(this.#moonSlot, TID_GAME_TOOLTIP_REMOVEJEWEL2);
      return;
    }
    const first = this.#jewelSlots[0];
    const last = this.#infoSlots[JEWEL_ROWS - 1];
    if (first && last) {
      const rows: Rect = { l: first.l, t: first.t, r: last.r, b: last.b };
      if (rectContains(rows, pt)) tip(rows, TID_GAME_TOOLTIP_REMOVEJEWEL3);
    }
  }

  // ---- Start --------------------------------------------------------------------------------------------------------------------

  /** Start: asks for the confirmation. */
  start(): void {
    if (this.#asking || !this.#weapon || !this.#moonstone) return;
    this.#asking = true;
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.env.wm.open(
      APP_SMELT_REMOVE_CONFIRM,
      () =>
        new WndRemoveJewelConfirm({
          text: this.#deps.request.confirmText,
          yes: () => {
            this.#send();
          },
          closed: () => {
            if (!this.#asking) return;
            this.#asking = false;
            this.#sync();
          },
        }),
    );
  }

  /** Yes: `SendUltimateRemoveGem( weapon, moonstone )` and the window closes (the items are released by `onDestroy`). */
  #send(): void {
    const w = this.#weapon;
    const m = this.#moonstone;
    if (w && m) this.#deps.request.remove(w.objId, m.objId);
    // the confirmation closes itself after Yes
    this.#asking = false;
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_START) {
      this.start();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    const req = this.#deps.request;
    if (this.#weapon) req.setExtra(this.#weapon.objId, 0);
    if (this.#moonstone) req.setExtra(this.#moonstone.objId, 0);
    const c = this.env.wm.get(APP_SMELT_REMOVE_CONFIRM);
    if (c && this.#asking) c.destroy();
    req.closed?.();
    super.onDestroy();
  }

  // ---- the pixels ---------------------------------------------------------------------------------------------------------------

  #sync(): void {
    this.getDlgItem(WIDC_START)?.enableWindow(
      !this.#asking && this.#weapon !== null && this.#moonstone !== null,
    );
    this.#paint();
  }

  #paint(): void {
    const req = this.#deps.request;
    const entries = [];
    // the moonstone slot always shows the moonstone: faint while it is empty (`Render( ..., 50 )`)
    entries.push(
      slotEntry('moonstone', req.moonstoneIcon, this.#moonSlot, this.#moonstone ? 255 : 50),
    );
    const texts: TextItem[] = [];
    const w = this.#weapon;
    if (w) {
      entries.push(slotEntry('weapon', w.view.icon, this.#weaponSlot));
      const rows = Math.min(w.weapon?.sockets ?? 0, JEWEL_ROWS);
      for (let i = 0; i < rows; i += 1) {
        const gem = w.weapon?.gems[i] ?? 0;
        if (gem === 0) continue;
        const slot = this.#jewelSlots[i];
        const info = this.#infoSlots[i];
        if (slot) entries.push(slotEntry(`jewel${String(i)}`, req.jewelIcon(gem), slot));
        const label = req.jewelLabel(gem);
        if (info && label) {
          texts.push({ x: info.l + 10, y: info.t + 10, text: label.text, color: label.color });
        }
      }
    }
    this.#strip?.setEntries(entries);
    this.#texts?.setItems(texts);
  }
}

// ---- APP_SMELT_REMOVE_CONFIRM -------------------------------------------------------------------------------------------------------

export interface RemoveJewelConfirmDeps {
  /** `prj.GetText( TID_GAME_REMOVEJEWEL )`. */
  readonly text: string;
  /** Yes. */
  yes(): void;
  /** The window closed, by any button. */
  closed?(): void;
}

/**
 * `CWndRemoveJewelConfirm` (`WndField.cpp:21524-21605`): `APP_SMELT_REMOVE_CONFIRM` (288 x 128), "Confirm Jewel Removal": the warning text, Yes and
 * No; the close button counts as No.
 */
export class WndRemoveJewelConfirm extends WndNeuz {
  readonly appId = APP_SMELT_REMOVE_CONFIRM;
  readonly #deps: RemoveJewelConfirmDeps;

  constructor(deps: RemoveJewelConfirmDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setString(this.#deps.text);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_YES) {
      this.#deps.yes();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && (id === WIDC_NO || id === WTBID_CLOSE)) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#deps.closed?.();
    super.onDestroy();
  }
}
