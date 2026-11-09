// `WndGHTeleporter` (`CWndGHTeleporter`, `src:_Interface/WndGuildHouse.cpp:230-632`, Plan/42 §19.4 N, 45 §28.10; WP-UI-58): `APP_GH_TELEPORTER` (272 x 208), the five-slot teleporter window. V15 NEVER instantiates it (the teleporter
// of the guild house is the navigator's `MMI_TELEPORTER` and the storage window's teleporter section): this is a SHELL (`reachable: false` in the notes) that builds from the real layout and talks to its request.
//
//   * `SLOT1..5` hold the teleporter items in order (a `null` slot is empty); a press on a slot selects it (the fill `0x55ff2277`), the installed one is tinted yellow (`0xffffff00`);
//   * `BUTTON_OK` asks to place the selected teleporter (`SETUP`), `BUTTON_FIRE` or a double click on a slot asks to drop it (`LISTDROP`); the selection ends with either;
//   * an inventory teleporter dropped on the window is used (`DoUseItem`); `WIDC_TIME` reads the remaining time of the selected slot.

import {
  APP_GH_TELEPORTER,
  WIDC_BUTTON_FIRE,
  WIDC_BUTTON_OK,
  WIDC_SLOT1,
  WIDC_SLOT2,
  WIDC_SLOT3,
  WIDC_SLOT4,
  WIDC_SLOT5,
  WIDC_TIME,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, rectContains, rectHeight, rectWidth } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { HousingHost, TeleporterRequest, TeleporterSlot } from './ports';
import { fmt, timeSpan } from './rows';
import { APP_INVENTORY } from '../../../gen/defs/resdata';
import { TID_PK_LIMIT_DAY, TID_PK_LIMIT_HOUR, TID_PK_LIMIT_MINUTE } from '../../../gen/defs/text';

/** `MAX_TELEPORTER_SLOT`. */
export const TELEPORTER_SLOTS = 5;
const SLOT_IDS = [WIDC_SLOT1, WIDC_SLOT2, WIDC_SLOT3, WIDC_SLOT4, WIDC_SLOT5] as const;
const ID_RECTS = 0x7a50;
const ID_ICONS = 0x7a51;

/** The time text of a teleporter slot (the storage window's rule). */
export function teleporterTimeText(
  s: TeleporterSlot,
  now: number,
  text: (tid: number) => string,
): string {
  const seconds = s.expireAt <= 0 ? s.abilityMin * 60 : Math.max(0, s.expireAt - now);
  const ts = timeSpan(seconds);
  if (ts.days > 0) return fmt(text(TID_PK_LIMIT_DAY), [ts.days]);
  if (ts.hours > 0) return fmt(text(TID_PK_LIMIT_HOUR), [ts.hours]);
  return fmt(text(TID_PK_LIMIT_MINUTE), [ts.minutes]);
}

export class WndGHTeleporter extends WndNeuz {
  readonly appId = APP_GH_TELEPORTER;
  readonly #request: TeleporterRequest;
  readonly #host: HousingHost;
  #selected = -1;
  #rects: RectLayer | null = null;
  #icons: SpriteLayer | null = null;

  constructor(request: TeleporterRequest, host: HousingHost) {
    super();
    this.#request = request;
    this.#host = host;
    this.putRegInfo = false;
  }

  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON_OK);
  }
  get fireButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON_FIRE);
  }
  /** `m_nSelectedSlot`, -1 for none. */
  get selected(): number {
    return this.#selected;
  }
  get slotRects(): readonly RectEntry[] {
    return this.#rects?.entries ?? [];
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.getClientRect(false);
    this.#rects = this.addControl(new RectLayer(), ID_RECTS, client);
    this.#icons = this.addControl(new SpriteLayer(), ID_ICONS, client);
    this.refresh();
    this.moveParentCenter();
  }

  /** `RefreshItems()`: the icons, the tint of the installed slot, the selection and the time. */
  refresh(): void {
    const slots = this.#request.slots();
    const icons: SpriteEntry[] = [];
    const rects: RectEntry[] = [];
    SLOT_IDS.forEach((id, i) => {
      const r = this.getWndCtrl(id)?.rect;
      const s = slots[i];
      if (!r) return;
      if (s) {
        icons.push({
          key: `s${String(i)}`,
          path: s.icon === '' ? null : s.icon,
          x: r.l,
          y: r.t,
          w: rectWidth(r),
          h: rectHeight(r),
        });
        if (s.installed) {
          rects.push({
            key: `i${String(i)}`,
            x: r.l,
            y: r.t,
            w: rectWidth(r),
            h: rectHeight(r),
            fill: 'rgba(255, 255, 0, 0.33)',
          });
        }
      }
      if (i === this.#selected) {
        rects.push({
          key: 'sel',
          x: r.l,
          y: r.t,
          w: rectWidth(r),
          h: rectHeight(r),
          fill: 'rgba(255, 34, 119, 0.333)',
        });
      }
    });
    this.#icons?.setEntries(icons);
    this.#rects?.setEntries(rects);
    const sel = slots[this.#selected] ?? null;
    this.getDlgItem<Static>(WIDC_TIME)?.setTitle(
      sel ? teleporterTimeText(sel, this.#request.nowSec(), this.#host.text) : '',
    );
  }

  #slotAt(pt: Point): number {
    for (let i = 0; i < TELEPORTER_SLOTS; i += 1) {
      const r = this.getWndCtrl(SLOT_IDS[i] as number)?.rect;
      if (r && rectContains(r, pt)) return i;
    }
    return -1;
  }

  override onLButtonDown(pt: Point): void {
    const i = this.#slotAt(pt);
    if (i < 0) return;
    this.#selected = i;
    this.refresh();
  }

  override onLButtonDblClk(pt: Point): void {
    const i = this.#slotAt(pt);
    if (i >= 0 && this.#request.slots()[i]) this.#request.fire(i);
  }

  override onDropIcon(sc: Shortcut): boolean {
    if (sc.kind !== SC.ITEM || sc.from?.getFrameWnd()?.appId !== APP_INVENTORY) return false;
    this.#request.dropped(sc.id);
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && (id === WIDC_BUTTON_OK || id === WIDC_BUTTON_FIRE)) {
      const i = this.#selected;
      if (i >= 0 && this.#request.slots()[i]) {
        if (id === WIDC_BUTTON_OK) this.#request.setup(i);
        else this.#request.fire(i);
        this.#selected = -1;
        this.refresh();
      }
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
