// `WndQueryEquip` (`CWndQueryEquip`, `src:_Interface/WndField.cpp:521-990`; ref/22 §17 D, Plan/42 WP-UI-36, 45 §9.8):
// `APP_QUERYEQUIP` (236 x 284, "View"), the equipment of another player. It is the equipment half of the inventory window: the worn items
// of the inspected mover in the 19 placeholders (`WIDC_CUSTOM1..19`, the same part table as `CWndInventory`, scales 0.8 / 0.9 / 1 and
// the ghosted two-hander), the 3-D model in `WIDC_CUSTOM20` (dragged with the pointer to turn it) and the item tooltips with the flag
// `APP_QUERYEQUIP` (the requirement lines are the INSPECTED player's: the tooltip feature knows the owner). Nothing is clickable.
// The window closes when the inspected mover is gone (`Process`: `IsInvalidObj( pMover )` -> `Destroy()`); a second inspection opens
// a new window (the original `SAFE_DELETE`d the old one).

import { APP_QUERYEQUIP } from '../../../gen/defs/resdata';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll, type PaperLayer } from '../char/PaperDoll';
import { buildPaperLayers, partRects, slotPartAt } from '../char/inventoryLayout';
import type { CorePreviewHandle } from '../char/ports';
import { ROTATE_PER_PX } from '../char/WndInventory';
import type { QueryEquipPorts, QueryEquipView } from './ports';

export interface QueryEquipDeps {
  readonly ports: QueryEquipPorts;
  readonly tooltip: Pick<TooltipService, 'putItem'>;
}

export class WndQueryEquip extends WndNeuz {
  readonly appId = APP_QUERYEQUIP;
  readonly #deps: QueryEquipDeps;
  #rects: ReadonlyMap<number, Rect> = new Map();
  #model: Rect | null = null;
  #paper: PaperDoll | null = null;
  #preview: CorePreviewHandle | null = null;
  #view: QueryEquipView | null = null;
  #rot = 0;
  #rotating = false;
  #oldX = 0;
  #hoverPart: number | null = null;
  #scoped = false;

  constructor(deps: QueryEquipDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get paperDoll(): PaperDoll | null {
    return this.#paper;
  }
  get rotation(): number {
    return this.#rot;
  }
  get equipView(): QueryEquipView | null {
    return this.#view;
  }
  /** The rect of a part's placeholder in client coordinates (the e2e clicks use it). */
  partRect(part: number): Rect | null {
    return this.#rects.get(part) ?? null;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const custom = new Map<string, Rect>();
    for (let i = 1; i <= 20; i += 1) {
      const name = `WIDC_CUSTOM${String(i)}`;
      const c = this.getWndCtrl(name as 'WIDC_CUSTOM1');
      if (c) custom.set(name, { ...c.rect });
    }
    this.#rects = partRects((n) => custom.get(n) ?? null);
    this.#model = custom.get('WIDC_CUSTOM20') ?? null;
    // the paper doll under the controls of the layout
    const paper = new PaperDoll();
    this.addControl(paper, 0x7a31, this.getClientRect(false));
    this.controlList.splice(this.controlList.indexOf(paper), 1);
    this.controlList.unshift(paper);
    this.publish();
    this.#paper = paper;
    const v = this.#deps.ports.view.get();
    const canvas = this.#model ? this.customCanvas('WIDC_CUSTOM20') : null;
    if (canvas && this.#deps.ports.previews) {
      this.#preview = this.#deps.ports.previews.open({
        canvas,
        preset: 'inventory',
        subject: v.subject,
      });
    }
    this.#apply(v);
    this.moveParentCenter();
  }

  // ---- the view ---------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    super.process(now);
    const v = this.#deps.ports.view.get();
    if (v.gone) {
      this.destroy();
      return;
    }
    if (v !== this.#view) this.#apply(v);
  }

  #apply(v: QueryEquipView): void {
    this.#view = v;
    const layers: PaperLayer[] = buildPaperLayers({
      equip: v.equip,
      rects: this.#rects,
      hideCoat: v.hideCoat,
      newFontColor: true,
    });
    this.#paper?.setLayers(layers, true);
    this.#preview?.invalidate();
  }

  // ---- the pointer: turning the model, tooltips -------------------------------------------------------------------------

  override onLButtonDown(pt: Point): void {
    this.setCapture();
    const model = this.#model;
    if (model && rectContains(model, pt)) {
      this.#rotating = true;
      this.#oldX = pt.x;
    }
  }

  override onLButtonUp(): void {
    this.#rotating = false;
    this.releaseCapture();
  }

  override onPointerRelease(): void {
    this.#rotating = false;
  }

  override onMouseMove(pt: Point): void {
    if (this.#rotating) {
      this.#rot += (this.#oldX - pt.x) * ROTATE_PER_PX;
      this.#oldX = pt.x;
      this.#preview?.setRotation?.(this.#rot);
    }
    this.#hover(pt);
  }

  override onMouseLeave(): void {
    this.#hoverPart = null;
    this.#scope(false);
  }

  /** `OnMouseWndSurface`: the item of the placeholder under the pointer (parts 2.. with an item). */
  #hover(pt: Point): void {
    const v = this.#view;
    if (!v) return;
    const part = slotPartAt(pt, this.#rects);
    if (part === this.#hoverPart) return;
    this.#hoverPart = part;
    if (part === null) return;
    const item = v.equip[part] ?? null;
    const r = this.#rects.get(part);
    if (!item || !r) return;
    const o = this.getScreenClientRect();
    this.#scope(true);
    this.#deps.tooltip.putItem(this, item, rectOffset(r, o.l, o.t), APP_QUERYEQUIP, {
      version: v.version,
    });
  }

  /** The tooltip owner is the inspected player while the pointer is over an item of this window. */
  #scope(on: boolean): void {
    if (this.#scoped === on) return;
    this.#scoped = on;
    this.#deps.ports.tooltipScope?.(on);
  }

  override onDestroy(): void {
    this.#preview?.close();
    this.#preview = null;
    this.#scope(false);
    this.#deps.ports.closed?.();
    super.onDestroy();
  }
}
