// `WndRemovePiercing` (`CWndRemovePiercing`, `src:_Interface/WndField.cpp:22722-23010`, `__PIERCING_REMOVE`, ref/22 §17 L, Plan/45 §18.6; WP-UI-31 /
// WP-CFEAT-26): `APP_SMELT_REMOVE_PIERCING_EX` (272 x 480, `__CSC_VER12_4`, the live window; `MMI_SMELT_REMOVE_PIERCING`) or the older
// `APP_SMELT_REMOVE_PIERCING` (272 x 208, no socket lines). A slot (`WIDC_PIC_SLOT`) takes a pierced item (`IsPierceAble() && GetPiercingItem(0) != 0`, else
// the chat line `TID_GAME_REMOVE_PIERCING_ERROR_EX`); the description is `SmeltRemovePiercing.inc`; Start (`WIDC_START`) sends `PiercingRemove{item}`
// (the LAST card goes, 1 000 000 penya) and closes the window. The extended window lists the sockets of the item in ten rows
// (`WIDC_STATIC_PIERCING1..10`): the text of each card's options at (+10, +8) in black, and on a suit the rows 5..10 are shaded red
// (`ARGB(60, 240, 0, 0)`) because a suit has four sockets. The tooltip over the slot is `TID_GAME_TOOLTIP_PIERCINGITEM`.

import {
  APP_SMELT_REMOVE_PIERCING,
  APP_SMELT_REMOVE_PIERCING_EX,
  WIDC_PIC_SLOT,
  WIDC_START,
  WIDC_STATIC_PIERCING1,
  WIDC_STATIC_PIERCING10,
  WIDC_STATIC_PIERCING2,
  WIDC_STATIC_PIERCING3,
  WIDC_STATIC_PIERCING4,
  WIDC_STATIC_PIERCING5,
  WIDC_STATIC_PIERCING6,
  WIDC_STATIC_PIERCING7,
  WIDC_STATIC_PIERCING8,
  WIDC_STATIC_PIERCING9,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { DialogItem } from '../dialogs/ports';
import type { RemovePiercingRequest } from './ports';
import { NO_RECT, SlotWindow } from './SlotWindow';

const INFO_IDS = [
  WIDC_STATIC_PIERCING1,
  WIDC_STATIC_PIERCING2,
  WIDC_STATIC_PIERCING3,
  WIDC_STATIC_PIERCING4,
  WIDC_STATIC_PIERCING5,
  WIDC_STATIC_PIERCING6,
  WIDC_STATIC_PIERCING7,
  WIDC_STATIC_PIERCING8,
  WIDC_STATIC_PIERCING9,
  WIDC_STATIC_PIERCING10,
] as const;

/** `D3DCOLOR_ARGB( 60, 240, 0, 0 )`. */
const UNUSABLE_FILL = 'rgba(240, 0, 0, 0.235)';
/** `D3DCOLOR_ARGB(255,0,0,0)`. */
const OPTION_COLOR = 0xff000000;

export class WndRemovePiercing extends SlotWindow<RemovePiercingRequest> {
  readonly appId: number;
  readonly #tooltip: TooltipService | null;
  #infoSlots: Rect[] = [];
  #text: TextLayer | null = null;
  #shade: RectLayer | null = null;
  #sent = false;

  constructor(request: RemovePiercingRequest, tooltip?: TooltipService) {
    super(request, {
      slotId: WIDC_PIC_SLOT,
      startId: WIDC_START,
      textId: WIDC_TEXT1,
      description: request.description,
    });
    this.appId = request.extended ? APP_SMELT_REMOVE_PIERCING_EX : APP_SMELT_REMOVE_PIERCING;
    this.#tooltip = tooltip ?? null;
  }

  get textLayer(): TextLayer | null {
    return this.#text;
  }
  get shadeLayer(): RectLayer | null {
    return this.#shade;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    if (this.request.extended) {
      this.#infoSlots = INFO_IDS.map((id) => this.getWndCtrl(id)?.rect ?? NO_RECT);
      const client = this.getClientRect(false);
      this.#shade = this.addControl(new RectLayer(), 0x7e52, client);
      this.#text = this.addControl(new TextLayer(), 0x7e53, client);
    }
    this.#paintSockets();
  }

  override setItem(objId: number): boolean {
    const ok = super.setItem(objId);
    this.#paintSockets();
    return ok;
  }

  override clearItem(): void {
    super.clearItem();
    this.#paintSockets();
  }

  /** `OnMouseWndSurface`: the tooltip of the slot (the extended window). */
  override onMouseMove(pt: Point): void {
    super.onMouseMove(pt);
    if (!this.request.extended || !this.#tooltip || !rectContains(this.slotRect, pt)) return;
    const s = this.getScreenClientRect();
    this.#tooltip.put(this, this.request.slotTip, rectOffset(this.slotRect, s.l, s.t), 1);
  }

  protected onStart(item: DialogItem): void {
    if (this.#sent) return;
    this.#sent = true;
    this.startButton?.enableWindow(false);
    this.request.send(item.objId);
    this.destroy();
  }

  /** The ten rows: card texts and the red shade of the sockets a suit does not have. */
  #paintSockets(): void {
    if (!this.request.extended) return;
    const item = this.item;
    const shade: RectEntry[] = [];
    const lines: TextItem[] = [];
    if (item !== null) {
      const view = this.request.view(item.objId);
      for (let i = view.max; i < INFO_IDS.length; i += 1) {
        const r = this.#infoSlots[i];
        if (!r) continue;
        shade.push({
          key: `off${String(i)}`,
          x: r.l,
          y: r.t,
          w: r.r - r.l,
          h: r.b - r.t,
          fill: UNUSABLE_FILL,
        });
      }
      for (let i = 0; i < view.lines.length && i < view.max; i += 1) {
        const text = view.lines[i];
        const r = this.#infoSlots[i];
        if (!r || !text) continue;
        lines.push({ x: r.l + 10, y: r.t + 8, text, color: OPTION_COLOR });
      }
    }
    this.#shade?.setEntries(shade);
    this.#text?.setItems(lines);
  }
}
