// `WndChangeWeapon` (`CWndChangeWeapon`, `src:_Interface/WndField.cpp:21093-21520`, ref/22 §17 L, Plan/42 §19.4 L, 45 §18.6; WP-UI-32 /
// WP-CFEAT-26): `APP_SMELT_CHANGEWEAPON` (224 x 384), the weapon change. The type picks the window: a general weapon becomes a unique one
// (`TID_GAME_CHANGEWEAPON_UNIQUE`), a unique weapon an ultimate one (`TID_GAME_CHANGEWEAPON_ULTIMATE`). Three 32 x 32 slots: the weapon
// (`WIDC_STATIC1`: level 60 or more and the tier of the window; a unique weapon must be +10), the shining Orichalcum (`WIDC_STATIC2`) and the
// jewel of the weapon's level (`WIDC_STATIC3`, only after the weapon: `GetGemKind( limit_level )`). The two text boxes show the description
// and the information of the type. Items are dropped on their slot or double-clicked in the bag (`feed`: the weapon first, then the Orichalcum,
// then the jewel); a double click on a slot gives the item back. Start is enabled with all three slots filled and no request out; it sends
// `UltimateTransWeapon{weapon, jewel, shining}`. Every answer empties the slots and releases the items; success and failure print the line of
// the type, cancel closes the window, a full bag prints `TID_GAME_EXTRACTION_ERROR`. The tooltips are plain texts over the three slots.
//
// KEEP-DEVIATION: the original lets a second drop on the Orichalcum or jewel slot replace the stack and leaks the reservation of the first;
// here an occupied slot ignores the drop (like the weapon slot does).

import {
  APP_SMELT_CHANGEWEAPON,
  WIDC_START,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_TEXT1,
  WIDC_TEXT2,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_CGWEAPON_FAILED,
  TID_GAME_CGWEAPON_SUCCESS,
  TID_GAME_CHANGEWEAPON_ULTIMATE,
  TID_GAME_CHANGEWEAPON_UNIQUE,
  TID_GAME_CUWEAPON_FAILED,
  TID_GAME_CUWEAPON_SUCCESS,
  TID_GAME_EXTRACTION_ERROR,
  TID_TOOLTIP_CHANGEW_GENERAL,
  TID_TOOLTIP_CHANGEW_JEWEL,
  TID_TOOLTIP_CHANGEW_ORICALCUM,
  TID_TOOLTIP_CHANGEW_UNIQUE,
} from '../../../gen/defs/text';
import { IconStrip } from '../../controls/IconStrip';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { ANSWER_TIMEOUT_MS, CHANGE_MIN_LEVEL, UNIQUE_MIN_AO } from './constants';
import type { ChangeWeaponRequest, UltimateItem } from './ports';
import { ULTIMATE_CANCEL, ULTIMATE_FAILED, ULTIMATE_INVENTORY, ULTIMATE_SUCCESS } from './results';
import { NO_RECT, fromBag, slotEntry, toClient } from './slots';

/** The slots, in the order of `m_pWItemElem`, `m_pJItemElem[0]` (the Orichalcum), `m_pJItemElem[1]` (the jewel). */
type SlotName = 'weapon' | 'shining' | 'jewel';

export interface ChangeWeaponDeps {
  readonly request: ChangeWeaponRequest;
  readonly getText: (tid: number) => string;
  readonly tooltip: TooltipService;
}

export class WndChangeWeapon extends WndNeuz {
  readonly appId = APP_SMELT_CHANGEWEAPON;
  readonly #deps: ChangeWeaponDeps;
  readonly #items: Record<SlotName, UltimateItem | null> = {
    weapon: null,
    shining: null,
    jewel: null,
  };
  #rects: Record<SlotName, Rect> = { weapon: NO_RECT, shining: NO_RECT, jewel: NO_RECT };
  #strip: IconStrip | null = null;
  #text1: TextCtrl | null = null;
  #text2: TextCtrl | null = null;
  /** `m_bIsSendChange`. */
  #sent = false;
  /** `host.now()` of the send. */
  #sentAt = 0;

  constructor(deps: ChangeWeaponDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `m_nWeaponType`. */
  get type(): 'general' | 'unique' {
    return this.#deps.request.type;
  }
  /** The item in a slot, `null` when it is empty. */
  slot(name: SlotName): UltimateItem | null {
    return this.#items[name];
  }
  slotRect(name: SlotName): Rect {
    return this.#rects[name];
  }
  get strip(): IconStrip | null {
    return this.#strip;
  }
  get descriptionText(): TextCtrl | null {
    return this.#text1;
  }
  get infoText(): TextCtrl | null {
    return this.#text2;
  }
  get sent(): boolean {
    return this.#sent;
  }
  get startEnabled(): boolean {
    return this.getDlgItem(WIDC_START)?.isWindowEnabled() ?? false;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rect = (id: number): Rect => this.getWndCtrl(id)?.rect ?? NO_RECT;
    this.#rects = {
      weapon: rect(WIDC_STATIC1),
      shining: rect(WIDC_STATIC2),
      jewel: rect(WIDC_STATIC3),
    };
    const req = this.#deps.request;
    this.#text1 = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text2 = this.getDlgItem<TextCtrl>(WIDC_TEXT2);
    this.#text1?.setString(req.description);
    this.#text2?.setString(req.info);
    this.setTitle(
      this.#deps.getText(
        req.type === 'general' ? TID_GAME_CHANGEWEAPON_UNIQUE : TID_GAME_CHANGEWEAPON_ULTIMATE,
      ),
    );
    this.#strip = this.addControl(new IconStrip(), 0x7f31, this.getClientRect(false));
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#paint();
    this.moveParentCenter();
  }

  // ---- the slots ----------------------------------------------------------------------------------------------------------------

  /** The weapons the weapon slot takes: level 60 or more, the tier of the window, and +10 for a unique weapon. */
  canTakeWeapon(item: UltimateItem): boolean {
    const w = item.weapon;
    if (!w || w.level < CHANGE_MIN_LEVEL) return false;
    if (w.tier !== this.type) return false;
    return this.type === 'general' || w.ao >= UNIQUE_MIN_AO;
  }

  /** The jewel of the weapon in the slot (`GetGemKind( limit_level )`), `null` without a weapon or a table row. */
  wantedJewel(): number | null {
    const w = this.#items.weapon?.weapon;
    return w ? this.#deps.request.gemKind(w.level) : null;
  }

  /** Whether `item` fits `name` right now. */
  fits(name: SlotName, item: UltimateItem): boolean {
    if (this.#items[name] !== null) return false;
    switch (name) {
      case 'weapon':
        return this.canTakeWeapon(item);
      case 'shining':
        return item.role === 'shining';
      case 'jewel': {
        const want = this.wantedJewel();
        return want !== null && item.itemId === want;
      }
    }
  }

  /** `OnDropIcon`: an item onto one of the three slots. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (!fromBag(sc)) return false;
    const pt = toClient(this, ptWindow);
    const item = this.#deps.request.item(sc.id);
    if (!item) {
      // a worn item or one the bag does not have: the forbid cursor
      this.env.wm.drag.forbid();
      return false;
    }
    for (const name of ['weapon', 'shining', 'jewel'] as const) {
      if (rectContains(this.#rects[name], pt) && this.fits(name, item)) this.#put(name, item);
    }
    this.#sync();
    return true;
  }

  /** `SetItem` (the double click of a bag item, rule D3 `feed`): the weapon, else the Orichalcum, else the jewel. */
  feed(objId: number): boolean {
    const item = this.#deps.request.item(objId);
    if (!item) return false;
    for (const name of ['weapon', 'shining', 'jewel'] as const) {
      if (this.fits(name, item)) {
        this.#put(name, item);
        this.#sync();
        return true;
      }
    }
    return false;
  }

  #put(name: SlotName, item: UltimateItem): void {
    this.#items[name] = item;
    this.#deps.request.setExtra(item.objId, item.extra + 1);
  }

  /** `OnLButtonDblClk`: the item of the slot under the pointer goes back. */
  override onLButtonDblClk(pt: Point): void {
    for (const name of ['weapon', 'shining', 'jewel'] as const) {
      const it = this.#items[name];
      if (it !== null && rectContains(this.#rects[name], pt)) {
        this.#deps.request.setExtra(it.objId, 0);
        this.#items[name] = null;
      }
    }
    this.#sync();
  }

  /** `OnMouseWndSurface`: the plain tooltips over the three slots. */
  override onMouseMove(pt: Point): void {
    const s = this.getScreenClientRect();
    const tips: [SlotName, number][] = [
      [
        'weapon',
        this.type === 'general' ? TID_TOOLTIP_CHANGEW_GENERAL : TID_TOOLTIP_CHANGEW_UNIQUE,
      ],
      ['shining', TID_TOOLTIP_CHANGEW_ORICALCUM],
      ['jewel', TID_TOOLTIP_CHANGEW_JEWEL],
    ];
    for (const [name, tid] of tips) {
      const r = this.#rects[name];
      if (rectContains(r, pt)) {
        this.#deps.tooltip.put(this, this.#deps.getText(tid), rectOffset(r, s.l, s.t), 1);
        return;
      }
    }
  }

  // ---- Start and the answer -----------------------------------------------------------------------------------------------------

  /** Start: with all three slots filled and nothing out, `UltimateTransWeapon{weapon, jewel, shining}`. */
  start(): void {
    const { weapon, shining, jewel } = this.#items;
    if (this.#sent || !weapon || !shining || !jewel) return;
    this.#sent = true;
    this.#sentAt = this.env.host.now();
    this.#sync();
    this.#deps.request.transform(weapon.objId, jewel.objId, shining.objId);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_START) {
      this.start();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `Process`: a request without an answer for `ANSWER_TIMEOUT_MS` is given up (the slots keep their items). */
  override process(now: number): void {
    super.process(now);
    if (this.#sent && now - this.#sentAt >= ANSWER_TIMEOUT_MS) {
      this.#sent = false;
      this.#sync();
    }
  }

  /** `ReceiveResult`: the line of the result, then the slots are emptied whatever it was. */
  receiveResult(result: number): void {
    const unique = this.type === 'unique';
    switch (result) {
      case ULTIMATE_SUCCESS:
        this.#deps.request.say(unique ? TID_GAME_CUWEAPON_SUCCESS : TID_GAME_CGWEAPON_SUCCESS);
        break;
      case ULTIMATE_FAILED:
        this.#deps.request.say(unique ? TID_GAME_CUWEAPON_FAILED : TID_GAME_CGWEAPON_FAILED);
        break;
      case ULTIMATE_CANCEL:
        this.destroy();
        break;
      case ULTIMATE_INVENTORY:
        this.#deps.request.say(TID_GAME_EXTRACTION_ERROR);
        break;
      default:
        break;
    }
    for (const name of ['weapon', 'shining', 'jewel'] as const) {
      const it = this.#items[name];
      if (it) this.#deps.request.setExtra(it.objId, 0);
      this.#items[name] = null;
    }
    this.#sent = false;
    this.#sync();
  }

  override onDestroy(): void {
    for (const it of Object.values(this.#items)) if (it) this.#deps.request.setExtra(it.objId, 0);
    this.#deps.request.closed?.();
    super.onDestroy();
  }

  // ---- the pixels ---------------------------------------------------------------------------------------------------------------

  /** `Process`: Start is enabled with all three slots filled and no request out. */
  #sync(): void {
    const { weapon, shining, jewel } = this.#items;
    this.getDlgItem(WIDC_START)?.enableWindow(!this.#sent && !!weapon && !!shining && !!jewel);
    this.#paint();
  }

  #paint(): void {
    const entries = (['weapon', 'shining', 'jewel'] as const).flatMap((name) => {
      const it = this.#items[name];
      return it ? [slotEntry(name, it.view.icon, this.#rects[name])] : [];
    });
    this.#strip?.setEntries(entries);
  }
}
