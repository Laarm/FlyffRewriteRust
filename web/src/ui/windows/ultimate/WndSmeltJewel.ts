// `WndSmeltJewel` (`CWndSmeltJewel`, `src:_Interface/WndField.cpp:20470-21090`, ref/22 §17 L, Plan/42 §19.4 L, 45 §18.6; WP-UI-32 /
// WP-CFEAT-26): `APP_SMELT_JEWEL` (256 x 432), "Jewel Setting". An ultimate weapon is dropped on the picture (`WIDC_CUSTOM1`, the turning
// weapon of `WeaponStage`) and shows its five sockets (`WIDC_STATIC1..5`): a socket the weapon does not have is red, a filled one blue with
// the picture of its ability item, an empty one grey - except the first empty socket (`m_nUsableSlot`), which stays clear and takes the jewel
// (diamond, emerald, sapphire, ruby or topaz) dropped on it or double-clicked in the bag. A weapon without sockets takes no jewel. Start
// (enabled with a jewel in the free socket) turns the weapon for about six seconds (`spinTimeline`) and THEN sends
// `UltimateSetGem{weapon, jewel}`; the weapon cannot be changed meanwhile. The answer: success prints `TID_GAME_SMELTJEWEL_SUCCESS` and closes
// the window, failure prints `TID_GAME_SMELTJEWEL_FAIL` and reads the weapon again, cancel closes it. A double click on the free socket takes
// the jewel back. Weapon and jewel are reserved while they sit in the window and released when it closes.
//
// KEEP-DEVIATION: the weapon is read again from the bag after an answer (`req.item`): the original keeps a pointer to the live item, which
// the server's `UpdateItem` changed.

import {
  APP_SMELT_JEWEL,
  WIDC_CUSTOM1,
  WIDC_START,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { TID_GAME_SMELTJEWEL_FAIL, TID_GAME_SMELTJEWEL_SUCCESS } from '../../../gen/defs/text';
import { IconStrip } from '../../controls/IconStrip';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { SmeltJewelRequest, UltimateItem } from './ports';
import { ULTIMATE_CANCEL, ULTIMATE_FAILED, ULTIMATE_SUCCESS } from './results';
import { NO_RECT, SLOT_SIZE, fromBag, slotEntry, toClient } from './slots';
import { SOCKETS } from './constants';
import { spinTimeline } from './spin';
import { WeaponStage } from './WeaponStage';

const SOCKET_IDS = [WIDC_STATIC1, WIDC_STATIC2, WIDC_STATIC3, WIDC_STATIC4, WIDC_STATIC5] as const;
/** The overlays of a socket: not pierced, filled, empty (`WndDisableRed.bmp`, `WndDisableBlue.bmp`, `WndDisableBlack.bmp`). */
const OVERLAY = {
  closed: 'theme/WndDisableRed.bmp',
  filled: 'theme/WndDisableBlue.bmp',
  empty: 'theme/WndDisableBlack.bmp',
} as const;

export class WndSmeltJewel extends WndNeuz {
  readonly appId = APP_SMELT_JEWEL;
  readonly #req: SmeltJewelRequest;
  #weapon: UltimateItem | null = null;
  /** `m_dwJewel[5]`: the ability item of each socket, `0` = empty; the usable socket holds the new jewel's item id. */
  readonly #gems: number[] = [0, 0, 0, 0, 0];
  /** `m_nUsableSlot`: the first empty socket of the weapon, `-1` = none. */
  #usable = -1;
  /** `m_pJewelElem`: the jewel stack in the free socket. */
  #jewel: UltimateItem | null = null;
  #sockets: Rect[] = [];
  #canvas: Rect = NO_RECT;
  #strip: IconStrip | null = null;
  #overlay: SpriteLayer | null = null;
  #stage: WeaponStage | null = null;
  #text: TextCtrl | null = null;
  /** `m_nStatus == 1`: the weapon is turning. */
  #spinStart: number | null = null;
  readonly #spinMs = spinTimeline().ms;

  constructor(req: SmeltJewelRequest) {
    super();
    this.#req = req;
    this.putRegInfo = false;
  }

  /** The weapon in the window (its object id is what the dispatch asks for). */
  get weapon(): UltimateItem | null {
    return this.#weapon;
  }
  get jewel(): UltimateItem | null {
    return this.#jewel;
  }
  /** `m_nUsableSlot`. */
  get usableSlot(): number {
    return this.#usable;
  }
  /** `m_nStatus == 1`. */
  get spinning(): boolean {
    return this.#spinStart !== null;
  }
  /** How long the turn lasts. */
  get spinMs(): number {
    return this.#spinMs;
  }
  get socketRects(): readonly Rect[] {
    return this.#sockets;
  }
  get canvasRect(): Rect {
    return this.#canvas;
  }
  get strip(): IconStrip | null {
    return this.#strip;
  }
  get overlayLayer(): SpriteLayer | null {
    return this.#overlay;
  }
  get stage(): WeaponStage | null {
    return this.#stage;
  }
  get descriptionText(): TextCtrl | null {
    return this.#text;
  }
  /** The ability item of socket `i` (or the new jewel in the usable one), `0` for an empty socket. */
  socketItem(i: number): number {
    return this.#gems[i] ?? 0;
  }
  get startEnabled(): boolean {
    return this.getDlgItem(WIDC_START)?.isWindowEnabled() ?? false;
  }

  /** The jewels the sockets take: diamond, emerald, sapphire, ruby, topaz. */
  static isJewel(item: UltimateItem): boolean {
    return item.role === 'jewel';
  }

  /** An ultimate weapon (`IK2_WEAPON_DIRECT` / `_MAGIC` and `WEAPON_ULTIMATE`). */
  static isUltimateWeapon(item: UltimateItem): boolean {
    return item.weapon !== null && item.weapon.tier === 'ultimate' && item.weapon.directOrMagic;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#sockets = SOCKET_IDS.map((id) => this.getWndCtrl(id)?.rect ?? NO_RECT);
    this.#canvas = this.getWndCtrl(WIDC_CUSTOM1)?.rect ?? NO_RECT;
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#req.description);
    const client = this.getClientRect(false);
    this.#stage = this.addControl(new WeaponStage(), 0x7f41, this.#canvas);
    this.#overlay = this.addControl(new SpriteLayer(), 0x7f42, client);
    this.#strip = this.addControl(new IconStrip(), 0x7f43, client);
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#paint();
    this.moveParentCenter();
  }

  // ---- the weapon and the jewel -------------------------------------------------------------------------------------------------

  /** `InitializeJewel( pItemElem )`: reads the sockets of `weapon` and finds the first free one. */
  #initialize(weapon: UltimateItem): void {
    this.#weapon = weapon;
    this.#jewel = null;
    this.#gems.fill(0);
    const w = weapon.weapon;
    let count = 0;
    const size = Math.min(w?.sockets ?? 0, SOCKETS);
    for (let i = 0; i < size; i += 1) {
      const gem = w?.gems[i] ?? 0;
      if (gem !== 0) {
        this.#gems[i] = gem;
        count += 1;
      }
    }
    this.#usable = count < size ? count : -1;
  }

  /** `OnDropIcon`: an ultimate weapon on the picture, a jewel on the free socket. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (this.spinning || !fromBag(sc)) return false;
    const pt = toClient(this, ptWindow);
    const item = this.#req.item(sc.id);
    if (!item) {
      // a worn item or one the bag does not have: the forbid cursor
      this.env.wm.drag.forbid();
      return false;
    }
    if (WndSmeltJewel.isUltimateWeapon(item) && rectContains(this.#canvas, pt)) {
      this.#setWeapon(item);
    }
    if (WndSmeltJewel.isJewel(item) && this.#canJewel()) {
      const socket = this.#sockets.findIndex((r) => rectContains(r, pt));
      if (socket === this.#usable && socket >= 0) this.#setJewel(item);
    }
    this.#sync();
    return true;
  }

  /** The weapon replaces the previous one (its reservation goes), `m_pItemElem->SetExtra( GetExtra() + 1 )`. */
  #setWeapon(item: UltimateItem): void {
    if (this.#weapon) this.#req.setExtra(this.#weapon.objId, 0);
    if (this.#jewel) this.#req.setExtra(this.#jewel.objId, 0);
    this.#initialize(item);
    this.#req.setExtra(item.objId, item.extra + 1);
  }

  /** `GetUltimatePiercingSize() > 0`, the free socket is empty. */
  #canJewel(): boolean {
    return (
      this.#weapon !== null &&
      (this.#weapon.weapon?.sockets ?? 0) > 0 &&
      this.#usable > -1 &&
      (this.#gems[this.#usable] ?? 0) === 0
    );
  }

  #setJewel(item: UltimateItem): void {
    if (this.#usable < 0) return;
    this.#gems[this.#usable] = item.itemId;
    this.#jewel = item;
    this.#req.setExtra(item.objId, item.extra + 1);
  }

  /** `SetWeapon` / `SetJewel` for the double click of a bag item (rule D3 `feed`). */
  feed(objId: number): boolean {
    const item = this.#req.item(objId);
    if (!item || this.spinning) return false;
    if (WndSmeltJewel.isJewel(item) && this.#canJewel()) {
      this.#setJewel(item);
      this.#sync();
      return true;
    }
    return false;
  }

  /** `OnLButtonDblClk` on the free socket gives the jewel back. */
  override onLButtonDblClk(pt: Point): void {
    if (this.spinning || this.#usable < 0) return;
    const r = this.#sockets[this.#usable];
    if (!r || !rectContains(r, pt)) return;
    if ((this.#gems[this.#usable] ?? 0) !== 0 && this.#jewel) {
      this.#req.setExtra(this.#jewel.objId, 0);
      this.#jewel = null;
      this.#gems[this.#usable] = 0;
      this.#sync();
    }
  }

  // ---- Start, the turn and the answer -------------------------------------------------------------------------------------------

  /** Start: the weapon turns; `process` sends the request when the turn is over. */
  start(now: number = this.env.host.now()): void {
    if (this.spinning || !this.startEnabled) return;
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#spinStart = now;
    this.#stage?.setSpinning(true);
    this.#req.started?.();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_START) {
      this.start();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `Process`: the turn ends after `spinMs` and the request goes out. */
  override process(now: number): void {
    super.process(now);
    if (this.#spinStart === null || now - this.#spinStart < this.#spinMs) return;
    this.#spinStart = null;
    this.#stage?.setSpinning(false);
    const w = this.#weapon;
    const j = this.#jewel;
    if (w && j) this.#req.set(w.objId, j.objId);
    this.#sync();
  }

  /** `ReceiveResult`. */
  receiveResult(result: number): void {
    // the answer ends a turn that is still running (a stale request)
    this.#spinStart = null;
    this.#stage?.setSpinning(false);
    switch (result) {
      case ULTIMATE_SUCCESS:
        this.#req.say(TID_GAME_SMELTJEWEL_SUCCESS);
        this.#reinitialize();
        this.destroy();
        break;
      case ULTIMATE_FAILED:
        this.#req.say(TID_GAME_SMELTJEWEL_FAIL);
        this.#reinitialize();
        break;
      case ULTIMATE_CANCEL:
        this.destroy();
        break;
      default:
        // `ISULTIMATE`, `INVENTORY`, `ISNOTULTIMATE`: the weapon is read again and the jewel is free (the original left the window waiting)
        this.#reinitialize();
        break;
    }
    this.#sync();
  }

  /** `InitializeJewel( m_pItemElem )` on the weapon as the bag has it now; the jewel's reservation goes. */
  #reinitialize(): void {
    const old = this.#weapon;
    if (!old) return;
    if (this.#jewel) this.#req.setExtra(this.#jewel.objId, 0);
    const fresh = this.#req.item(old.objId);
    if (fresh) this.#initialize(fresh);
    else {
      this.#weapon = null;
      this.#jewel = null;
      this.#gems.fill(0);
      this.#usable = -1;
    }
  }

  override onDestroy(): void {
    if (this.#jewel) this.#req.setExtra(this.#jewel.objId, 0);
    if (this.#weapon) this.#req.setExtra(this.#weapon.objId, 0);
    this.#req.closed?.();
    super.onDestroy();
  }

  // ---- the pixels ---------------------------------------------------------------------------------------------------------------

  /** `Process`: Start is enabled with a jewel in the free socket of the weapon and no turn running. */
  #sync(): void {
    const ready =
      this.#weapon !== null &&
      !this.spinning &&
      this.#usable >= 0 &&
      this.#usable < SOCKETS &&
      (this.#gems[this.#usable] ?? 0) !== 0;
    this.getDlgItem(WIDC_START)?.enableWindow(ready);
    this.#paint();
  }

  #paint(): void {
    const w = this.#weapon;
    this.#stage?.setIcon(w ? w.view.icon : null);
    const overlays: SpriteEntry[] = [];
    const icons = [];
    if (w) {
      const size = w.weapon?.sockets ?? 0;
      for (let i = 0; i < SOCKETS; i += 1) {
        const rect = this.#sockets[i];
        if (!rect) continue;
        const gem = this.#gems[i] ?? 0;
        let sprite: string | null = null;
        if (i >= size) sprite = OVERLAY.closed;
        else if (gem !== 0 && i !== this.#usable) sprite = OVERLAY.filled;
        else if (gem === 0 && i !== this.#usable) sprite = OVERLAY.empty;
        if (sprite !== null) {
          overlays.push({
            key: `overlay${String(i)}`,
            path: sprite,
            x: rect.l,
            y: rect.t,
            w: SLOT_SIZE,
            h: SLOT_SIZE,
          });
        }
        if (i < size && gem !== 0) {
          icons.push(slotEntry(`gem${String(i)}`, this.#req.iconOf(gem), rect));
        }
      }
    }
    this.#overlay?.setEntries(overlays);
    this.#strip?.setEntries(icons);
  }
}
