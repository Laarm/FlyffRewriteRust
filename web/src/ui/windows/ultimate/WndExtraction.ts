// `WndExtraction` (`CWndExtraction`, `src:_Interface/WndField.cpp:20236-20470`, ref/22 §17 L, Plan/42 §19.4 L, 45 §18.6; WP-UI-32 / WP-CFEAT-26):
// `APP_SMELT_EXTRACTION` (272 x 208), "Create Jewels". One 32 x 32 slot (`WIDC_PIC_SLOT`) takes a weapon: a general one of level 60 or more
// or any unique one (`dwReferStat1`); the description of `SmeltExtraction.inc` fills the text box. Start (enabled with a weapon in the slot)
// sends `UltimateMakeGem{weapon}` and disables itself. The answer (`receiveResult`): success prints `TID_GAME_EXTRACTION_SUCCESS` with the jewel
// name and the count and empties the slot (the weapon is gone); failure and cancel print `TID_GAME_EXTRACTION_FAILED` and close the window; a full
// bag prints `TID_GAME_EXTRACTION_ERROR` and enables Start again. A double click on the slot takes the weapon back; closing releases it.

import {
  APP_SMELT_EXTRACTION,
  WIDC_PIC_SLOT,
  WIDC_START,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_EXTRACTION_ERROR,
  TID_GAME_EXTRACTION_FAILED,
  TID_GAME_EXTRACTION_SUCCESS,
} from '../../../gen/defs/text';
import { IconStrip } from '../../controls/IconStrip';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { ANSWER_TIMEOUT_MS, canExtract } from './constants';
import type { ExtractionRequest, UltimateItem } from './ports';
import { ULTIMATE_CANCEL, ULTIMATE_FAILED, ULTIMATE_INVENTORY, ULTIMATE_SUCCESS } from './results';
import { NO_RECT, fromBag, slotEntry, toClient } from './slots';

export class WndExtraction extends WndNeuz {
  readonly appId = APP_SMELT_EXTRACTION;
  readonly #req: ExtractionRequest;
  #slot: Rect = NO_RECT;
  #strip: IconStrip | null = null;
  #text: TextCtrl | null = null;
  /** `m_pItemElem` / its level (`m_pEItemProp->dwLimitLevel1`, kept for the success text). */
  #weapon: UltimateItem | null = null;
  #level = 0;
  /** `host.now()` of the send, `null` = none out. */
  #sentAt: number | null = null;

  constructor(req: ExtractionRequest) {
    super();
    this.#req = req;
    this.putRegInfo = false;
  }

  get weapon(): UltimateItem | null {
    return this.#weapon;
  }
  get slotRect(): Rect {
    return this.#slot;
  }
  get strip(): IconStrip | null {
    return this.#strip;
  }
  get descriptionText(): TextCtrl | null {
    return this.#text;
  }
  get startEnabled(): boolean {
    return this.getDlgItem(WIDC_START)?.isWindowEnabled() ?? false;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#slot = this.getWndCtrl(WIDC_PIC_SLOT)?.rect ?? NO_RECT;
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#req.description);
    this.#strip = this.addControl(new IconStrip(), 0x7f21, this.getClientRect(false));
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#paint();
    this.moveParentCenter();
  }

  // ---- the slot -----------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: a weapon onto the slot; only into an empty slot and only the weapons of `canExtract`. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (!fromBag(sc)) return false;
    if (!rectContains(this.#slot, toClient(this, ptWindow))) return true;
    const item = this.#req.item(sc.id);
    if (!item) {
      // a worn item or one the bag does not have: the forbid cursor
      this.env.wm.drag.forbid();
      return false;
    }
    this.setWeapon(item);
    return true;
  }

  /** `SetWeapon` (the double click of a bag weapon, rule D3 `feed`, and the drop): `true` when the weapon went into the slot. */
  setWeapon(item: UltimateItem): boolean {
    if (this.#weapon !== null || !canExtract(item)) return false;
    this.#weapon = item;
    this.#level = item.weapon?.level ?? 0;
    this.#req.setExtra(item.objId, item.extra + 1);
    this.getDlgItem(WIDC_START)?.enableWindow(true);
    this.#paint();
    return true;
  }

  /** `SetWeapon` for the double click of a bag item (rule D3 `feed`). */
  feed(objId: number): boolean {
    const item = this.#req.item(objId);
    return item !== null && this.setWeapon(item);
  }

  /** `OnLButtonDblClk` on the slot gives the weapon back. */
  override onLButtonDblClk(pt: Point): void {
    if (!rectContains(this.#slot, pt) || this.#weapon === null) return;
    this.#req.setExtra(this.#weapon.objId, 0);
    this.#weapon = null;
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#paint();
  }

  // ---- Start and the answer -----------------------------------------------------------------------------------------------------

  /** Start: `UltimateMakeGem{weapon}`; the button waits for the answer. */
  start(): void {
    if (this.#weapon === null) return;
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#sentAt = this.env.host.now();
    this.#req.make(this.#weapon.objId);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_START) {
      this.start();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `Process`: a request without an answer for `ANSWER_TIMEOUT_MS` is given up (Start works again). */
  override process(now: number): void {
    super.process(now);
    if (this.#sentAt !== null && now - this.#sentAt >= ANSWER_TIMEOUT_MS) {
      this.#sentAt = null;
      this.getDlgItem(WIDC_START)?.enableWindow(this.#weapon !== null);
    }
  }

  /** `ReceiveResult( result, count )`. */
  receiveResult(result: number, count: number): void {
    this.#sentAt = null;
    switch (result) {
      case ULTIMATE_SUCCESS:
        this.#req.say(TID_GAME_EXTRACTION_SUCCESS, [this.#req.gemName(this.#level), count]);
        // the weapon was destroyed; the slot is free for the next one, Start waits for it
        this.#weapon = null;
        this.#paint();
        break;
      case ULTIMATE_FAILED:
      case ULTIMATE_CANCEL:
        this.#req.say(TID_GAME_EXTRACTION_FAILED);
        this.destroy();
        break;
      case ULTIMATE_INVENTORY:
        this.#req.say(TID_GAME_EXTRACTION_ERROR);
        this.getDlgItem(WIDC_START)?.enableWindow(true);
        break;
      default:
        // `ISULTIMATE`, `ISNOTULTIMATE` and the codes no window handles in the original: the request is over, Start works again
        this.getDlgItem(WIDC_START)?.enableWindow(this.#weapon !== null);
        break;
    }
  }

  /** `OnDestroy`: the weapon is released. */
  override onDestroy(): void {
    if (this.#weapon) this.#req.setExtra(this.#weapon.objId, 0);
    this.#req.closed?.();
    super.onDestroy();
  }

  #paint(): void {
    this.#strip?.setEntries(
      this.#weapon ? [slotEntry('weapon', this.#weapon.view.icon, this.#slot)] : [],
    );
  }
}
