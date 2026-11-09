// `MailItemSlot`: the 32 x 32 picture of the item that a mail carries - the slot of the send tab (`WIDC_STATIC3` of `APP_POST_SEND`: a drop target, a right
// click takes the item out again) and the slot of the reader (`WIDC_STATIC3` of `APP_POST_READ`: a press arms a drag to the inventory, a right click takes
// the item into the bag, the pointer over it frames it in red and shows the item tooltip). Both drew the icon at the top-left of the placeholder at alpha 255
// with the stack count at (right - 13, bottom - 13) in the outlined title font (`0xff0000ff`, then `0xffb0b0f0` one pixel up and left).

import { ITYPE_ITEM } from '../../../gen/defs/define';
import { viewOf } from '../../controls/bind';
import { useSprite } from '../../controls/useSprite';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { iconImageStyle } from '../../icons/IconCell';
import { type Rect, rectOffset } from '../../layout/geometry';
import { WTYPE_CUSTOM } from '../../layout/styles';
import { FONT, fontStyle } from '../../text/fonts';
import { Control, type ControlViewState } from '../../wnd/Control';
import type { InventoryItem } from '../char/ports';
import { useSyncExternalStore } from 'react';

export interface MailItemSlotState extends ControlViewState {
  item: InventoryItem | null;
  /** The slot has something to take (the item, or penya for the coin): the red frame of the reader shows on hover. */
  active: boolean;
  /** The number drawn at the corner (the attached amount, or the stack count of a mail's item); `null` = none. */
  count: number | null;
  /** The pointer is over the slot (the red frame of the reader). */
  hover: boolean;
}

export interface MailItemSlotDelegate {
  /** An inventory shortcut dropped on the slot. Returns whether it was taken. */
  drop?(sc: Shortcut): boolean;
  /** `OnRButtonUp` on the slot. */
  rightClick?(): void;
  /** The payload of a drag from the slot (`null` = not draggable); the slot sets `from`. */
  shortcutOf?(): Shortcut | null;
  /** The pointer entered (`rect` in root coordinates; `item` may be `null`: the coin) or left (`rect` is `null`) the slot. */
  hover?(item: InventoryItem | null, rect: Rect | null): void;
  /** The red round rect of the reader on hover. */
  frameOnHover?: boolean;
}

export class MailItemSlot extends Control<MailItemSlotState> {
  #delegate: MailItemSlotDelegate;

  constructor(delegate: MailItemSlotDelegate = {}) {
    super(WTYPE_CUSTOM, null, { item: null, active: false, count: null, hover: false });
    this.#delegate = delegate;
    this.view = viewOf<MailItemSlot>(MailItemSlotView);
  }

  get item(): InventoryItem | null {
    return this.state.get().item;
  }
  get count(): number | null {
    return this.state.get().count;
  }

  /** Shows `item` (`null` empties the slot) with `count` at the corner. */
  set(item: InventoryItem | null, count: number | null): void {
    this.state.patch({ item, count: item ? count : null, active: item !== null });
  }

  /** The coin slot of the reader: active while the mail carries penya. */
  setActive(active: boolean): void {
    this.state.patch({ active });
  }

  get frameOnHover(): boolean {
    return this.#delegate.frameOnHover === true;
  }

  override onDropIcon(sc: Shortcut): boolean {
    return this.#delegate.drop?.(sc) ?? false;
  }

  override onRButtonUp(): void {
    this.#delegate.rightClick?.();
  }

  override onLButtonDown(): void {
    if (this.env.wm.isDraggingShortcut() || !this.state.get().active) return;
    if (!this.#delegate.shortcutOf) return;
    this.env.wm.drag.arm(this, () => {
      const sc = this.#delegate.shortcutOf?.() ?? null;
      return sc ? { ...sc, from: this } : null;
    });
  }

  override onMouseMove(): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const item = this.state.get().item;
    if (!this.state.get().hover) {
      this.state.patch({ hover: true });
      const sc = this.getScreenRect();
      this.#delegate.hover?.(item, rectOffset({ l: 0, t: 0, r: 32, b: 32 }, sc.l, sc.t));
    }
  }

  override onMouseLeave(): void {
    if (this.state.get().hover) {
      this.state.patch({ hover: false });
      this.#delegate.hover?.(null, null);
    }
  }

  /** The payload helper for the reader: an item in the mail, `index = 0xffffffff` (no bag slot), `data = 1`. */
  static payload(item: InventoryItem): Shortcut {
    return {
      kind: SC.ITEM,
      id: item.objId,
      type: ITYPE_ITEM,
      index: 0xffff_ffff,
      userId: 0,
      data: 1,
      itemId: item.itemId,
      ...(item.icon === null ? {} : { icon: item.icon }),
    };
  }
}

export function MailItemSlotView({ control }: { control: MailItemSlot }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const sprite = useSprite(control.env, s.item ? s.item.icon : null);
  return (
    <div data-testid="mail-item-slot" style={{ position: 'absolute', inset: 0 }}>
      {s.item && (
        <div
          data-testid="mail-slot-icon"
          style={iconImageStyle({
            url: typeof sprite === 'object' ? sprite.url : null,
            size: 32,
            left: 0,
            top: 0,
            alpha: 1,
            tint: false,
          })}
        />
      )}
      {s.item && s.count !== null && s.item.stackable && (
        <>
          <div
            data-testid="mail-slot-count"
            style={{
              position: 'absolute',
              left: 19,
              top: 19,
              color: '#b0b0f0',
              whiteSpace: 'pre',
              textShadow: '1px 1px 0 #0000ff',
              ...fontStyle(FONT.title),
            }}
          >
            {String(s.count)}
          </div>
        </>
      )}
      {s.active && s.hover && control.frameOnHover && (
        <div
          data-testid="mail-slot-frame"
          style={{
            position: 'absolute',
            left: 0,
            top: 0,
            width: 32,
            height: 32,
            boxSizing: 'border-box',
            border: '1px solid rgb(150, 0, 0)',
            borderRadius: 3,
          }}
        />
      )}
    </div>
  );
}
