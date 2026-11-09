// `WndBuffPetStatus` (`CWndBuffPetStatus`, `src:_Interface/WndPetSys.cpp:2164-2845`, ref/22 §17 M, Plan/42 §19.4 M, 45 §18.7; WP-UI-34): `APP_BUFFPET_STATUS` (192 x 128),
// the window of an activated VIS pet (an eat pet that wears vis charms): opened by `VisPetActivate` with an item, closed by the one that sends none.
//   * the picture `BuffpetStatusBg .tga` (the original's file name has a blank before the extension) at the model, the model of the eat pet over `WIDC_CUSTOM1`
//     (41's preview of the pet mover) and nine 32 x 32 sockets (`WIDC_BUFFPET_SLOT1..9`);
//   * the first `size` sockets (`GetPiercingSize()`) are usable: a charm is drawn with its icon, dim (alpha 0x4f) when its prerequisites do not hold
//     (`GetValidVisTable`); the others show the lock `Icon_Lock.dds`;
//   * a charm from the bag dropped on a free usable socket asks `CVS_EQUIP_VIS` (the request opens the confirm); a charm dragged from one socket to another sends
//     `VisPetSwapVis{a, b}`; a double click on a socket with a charm asks `CVS_UNEQUIP_VIS`; the tooltip over a socket is the item tooltip with the keep time;
//   * the title is the name of the eat pet; the window sits at the right of the status window, or at its left when it would leave the layout, else 192 px
//     from the left of the layout.

import {
  APP_BUFFPET_STATUS,
  APP_INVENTORY,
  APP_STATUS1,
  WIDC_BUFFPET_SLOT1,
  WIDC_BUFFPET_SLOT2,
  WIDC_BUFFPET_SLOT3,
  WIDC_BUFFPET_SLOT4,
  WIDC_BUFFPET_SLOT5,
  WIDC_BUFFPET_SLOT6,
  WIDC_BUFFPET_SLOT7,
  WIDC_BUFFPET_SLOT8,
  WIDC_BUFFPET_SLOT9,
  WIDC_CUSTOM1,
} from '../../../gen/defs/resdata';
import { SpriteLayer } from '../../controls/SpriteLayer';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PreviewHandleLike } from '../login/ports';
import type { BuffPetRequest, BuffPetView } from './ports';

/** `MAX_VIS`. */
export const MAX_VIS = 9;
const SLOT_IDS = [
  WIDC_BUFFPET_SLOT1,
  WIDC_BUFFPET_SLOT2,
  WIDC_BUFFPET_SLOT3,
  WIDC_BUFFPET_SLOT4,
  WIDC_BUFFPET_SLOT5,
  WIDC_BUFFPET_SLOT6,
  WIDC_BUFFPET_SLOT7,
  WIDC_BUFFPET_SLOT8,
  WIDC_BUFFPET_SLOT9,
] as const;
export const BUFFPET_BG = 'theme/BuffpetStatusBg .tga';
export const LOCK_ICON = 'icon/Icon_Lock.dds';
/** `0x4fffffff` against `0xffffffff`: the colour of a charm whose prerequisites do not hold. */
export const INVALID_ALPHA = 0x4f / 255;
const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_BEHIND = 0x7a70;
const ID_SPRITES = 0x7a71;

export interface BuffPetDeps {
  readonly request: BuffPetRequest;
}

export class WndBuffPetStatus extends WndNeuz {
  readonly appId = APP_BUFFPET_STATUS;
  readonly #request: BuffPetRequest;
  #slots: Rect[] = Array.from({ length: MAX_VIS }, () => NO_RECT);
  #face: Rect = NO_RECT;
  #behind: SpriteLayer | null = null;
  #sprites: SpriteLayer | null = null;
  #preview: PreviewHandleLike | null = null;
  #view: BuffPetView | null = null;
  #applied: BuffPetView | null | undefined = undefined;

  constructor(deps: BuffPetDeps) {
    super();
    this.#request = deps.request;
    this.putRegInfo = false;
  }

  get slotRects(): readonly Rect[] {
    return this.#slots;
  }
  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  get background(): SpriteLayer | null {
    return this.#behind;
  }
  get previewHandle(): PreviewHandleLike | null {
    return this.#preview;
  }
  /** The socket index under a window point (`GetSlotIndexByPoint`), `-1` = none. */
  slotAt(pt: Point): number {
    return this.#slots.findIndex((r) => rectContains(r, pt));
  }
  /** `IsFreeSlot`: the socket exists and is not locked. */
  isFreeSlot(index: number): boolean {
    return index >= 0 && index < (this.#view?.size ?? 0);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#slots = SLOT_IDS.map((id) => this.getWndCtrl(id)?.rect ?? NO_RECT);
    this.#face = this.getWndCtrl(WIDC_CUSTOM1)?.rect ?? NO_RECT;
    const client = this.getClientRect(false);
    this.#behind = this.addControl(new SpriteLayer(), ID_BEHIND, client);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    const view = this.#request.view.get();
    const canvas = this.customCanvas(WIDC_CUSTOM1);
    if (canvas && view?.subject && this.#request.previews) {
      this.#preview = this.#request.previews.open({
        canvas,
        preset: 'buffPet',
        subject: view.subject,
      });
    }
    // beside the status window; to its left when the layout is not wide enough
    const status = this.env.wm.get(APP_STATUS1);
    const layout = this.env.wm.layoutRect;
    if (status) {
      const w = this.rectWindow.r - this.rectWindow.l;
      const x = status.rectWindow.r + w > layout.r ? status.rectWindow.l - w : status.rectWindow.r;
      this.move({ x, y: status.rectWindow.t });
    } else {
      this.move({ x: layout.l + 192, y: layout.t });
    }
    this.#apply(view);
  }

  // ---- the view ----------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#request.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #apply(v: BuffPetView | null): void {
    this.#applied = v;
    this.#view = v;
    if (v) {
      this.setTitle(v.title);
      if (v.subject) this.#preview?.setSubject(v.subject);
    }
    this.#paint();
    this.#preview?.invalidate();
  }

  #paint(): void {
    const v = this.#view;
    if (!v) {
      this.#sprites?.setEntries([]);
      this.#behind?.setEntries([]);
      return;
    }
    this.#behind?.setEntries([
      {
        key: 'bg',
        path: BUFFPET_BG,
        x: this.#face.l - 12,
        y: this.#face.t - 22,
        w: 192,
        h: 128,
      },
    ]);
    this.#sprites?.setEntries(
      this.#slots.map((r, i) => {
        if (i >= v.size) {
          return { key: `slot${String(i)}`, path: LOCK_ICON, x: r.l, y: r.t, w: 32, h: 32 };
        }
        const slot = v.slots[i];
        return {
          key: `slot${String(i)}`,
          path: slot?.icon ?? null,
          x: r.l,
          y: r.t,
          w: 32,
          h: 32,
          alpha: slot && !slot.valid ? INVALID_ALPHA : 1,
        };
      }),
    );
  }

  // ---- input -------------------------------------------------------------------------------------------------------

  /** `OnLButtonDown`: a socket with a charm becomes a drag source (the payload carries the socket in `data`). */
  override onLButtonDown(pt: Point): void {
    const i = this.slotAt(pt);
    const slot = i >= 0 ? this.#view?.slots[i] : undefined;
    if (i < 0 || !slot || slot.vis === 0 || slot.icon === null) return;
    this.env.wm.drag.arm(this, () => ({
      kind: SC.ITEM,
      id: 0,
      type: 0,
      index: slot.vis,
      userId: 0,
      data: i,
      from: this,
      icon: slot.icon ?? '',
    }));
  }

  /** `OnLButtonDblClk`: take the charm out (a confirm first). */
  override onLButtonDblClk(pt: Point): void {
    const i = this.slotAt(pt);
    const slot = i >= 0 ? this.#view?.slots[i] : undefined;
    if (i < 0 || !slot || slot.vis === 0) return;
    this.#request.askRemove(i, slot.vis);
  }

  /** `OnDropIcon`: a bag charm on a free socket asks to be set; a charm from another socket swaps. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    const pt = {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
    const target = this.slotAt(pt);
    if (target < 0 || !this.isFreeSlot(target)) return false;
    const frame = sc.from?.getFrameWnd();
    if (frame?.id === APP_INVENTORY) {
      if (sc.kind !== SC.ITEM || !this.#request.acceptsItem(sc.id)) return false;
      this.#request.askEquip(sc.id, this.#request.visItemOf(sc.id));
      return true;
    }
    if (frame?.id === APP_BUFFPET_STATUS) {
      // dropped back on its own socket: nothing
      if (sc.data === target) return false;
      this.#request.swap(sc.data, target);
      return true;
    }
    return false;
  }

  override onMouseMove(pt: Point): void {
    const i = this.slotAt(pt);
    if (i < 0 || this.env.wm.isDraggingShortcut()) return;
    const slot = this.#view?.slots[i];
    if (!slot || slot.vis === 0) return;
    const s = this.getScreenClientRect();
    this.#request.tooltip(i, rectOffset(this.#slots[i] ?? NO_RECT, s.l, s.t));
  }

  override onDestroy(): void {
    this.#preview?.close();
    this.#preview = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
