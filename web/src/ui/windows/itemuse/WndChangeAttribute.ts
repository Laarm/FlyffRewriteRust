// `WndChangeAttribute` (`CWndChangeAttribute`, `src:_Interface/WndField.cpp:23440-23660`, `__EXT_ENCHANT`, ref/22 §17 L, Plan/42 §19.4 L,
// 45 §18.3; WP-UI-30 / WP-CFEAT-15): `APP_CHANGE_ATTRIBUTE` (304 x 288), the scroll of attribute change (`II_SYS_SYS_SCR_SOKCHANG`, opened by the
// double click of the scroll, rule D6). One item slot (`WIDC_STATIC1`, 32 x 32) takes the item whose element is to change, the description
// of `ChangeAttribute.inc` fills the text box and five 24 x 24 icons (`WIDC_CUSTOM1..5`: fire, water, electricity, wind, earth; the sprites of
// `Icon_MonElemantkind.inc` at 1.5 x) choose the new element, the chosen one glows. OK sends `ChangeAttribute{item, scroll, attribute 1..5}`.
//   * the scroll is reserved (`SetExtra + 1`) while the window is open, the target while it sits in the slot; both are released when the
//     window closes (the destructor), the target also by OK and by a double click on the slot;
//   * only an inventory item can be dropped (else the forbid cursor); the item must be element-refinable and carry an element
//     (`IsEleRefineryAble && m_nResistAbilityOption > 0`), else `TID_GAME_NOTELEMENT` is printed; a second item does not replace the first;
//   * a press and release on an icon chooses it (`OnLButtonUp`), OK without an item or an element sends nothing but still closes the window;
//   * the tooltips are plain texts: `TID_TOOLTIP_CA_INSLOT` over the slot (an item or not), `_FIRE` ... `_EARTH` over the icons.

import {
  APP_CHANGE_ATTRIBUTE,
  APP_INVENTORY,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { RectLayer, argbCss, type RectEntry } from '../../controls/RectLayer';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { DialogItem } from '../dialogs/ports';
import type { ChangeAttributeRequest } from './ports';

export interface ChangeAttributeDeps {
  readonly request: ChangeAttributeRequest;
  readonly tooltip: TooltipService;
}

const ELEMENT_IDS = [WIDC_CUSTOM1, WIDC_CUSTOM2, WIDC_CUSTOM3, WIDC_CUSTOM4, WIDC_CUSTOM5] as const;
/** The element sprites are 16 x 16 and drawn at 1.5 x (24 x 24). */
const ELEMENT_SPRITE = 16;
const ELEMENT_ZOOM = 1.5;
/** `FillRect( rect, 0xffbb00ff, 0x00bb00ff )`: the glow round the chosen icon. */
const GLOW_COLOR = 0xffbb00ff;
const GLOW_THICKNESS = 8;

export class WndChangeAttribute extends WndNeuz {
  readonly appId = APP_CHANGE_ATTRIBUTE;
  readonly #deps: ChangeAttributeDeps;
  #target: DialogItem | null = null;
  /** `m_nAttributeNum`: 0..4, `-1` = none. */
  #chosen = -1;
  #slot: Rect = { l: 0, t: 0, r: 0, b: 0 };
  #elements: Rect[] = [];
  #items: PaperDoll | null = null;
  #sprites: SpriteLayer | null = null;
  #rects: RectLayer | null = null;
  #text: TextCtrl | null = null;
  #released = false;

  constructor(deps: ChangeAttributeDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get target(): DialogItem | null {
    return this.#target;
  }
  /** The chosen element, 0 = fire ... 4 = earth, `-1` = none. */
  get chosen(): number {
    return this.#chosen;
  }
  get slotRect(): Rect {
    return this.#slot;
  }
  get elementRects(): readonly Rect[] {
    return this.#elements;
  }
  get descriptionText(): TextCtrl | null {
    return this.#text;
  }
  get spriteLayer(): SpriteLayer | null {
    return this.#sprites;
  }
  get glowLayer(): RectLayer | null {
    return this.#rects;
  }
  get itemLayer(): PaperDoll | null {
    return this.#items;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rect = (id: number): Rect => this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 };
    this.#slot = rect(WIDC_STATIC1);
    this.#elements = ELEMENT_IDS.map((id) => rect(id));
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#deps.request.description);
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), 0x7e01, client);
    this.#rects = this.addControl(new RectLayer(), 0x7e02, client);
    this.#items = this.addControl(new PaperDoll(), 0x7e03, client);
    // `SetChangeItem`: the scroll is reserved while the window is open
    this.#deps.request.reserve(this.#deps.request.scroll.objId, true);
    this.#paint();
    this.moveParentCenter();
  }

  // ---- the slot -----------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: an inventory item onto the slot (`pTempElem` must be refinable and carry an element). */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM) {
      this.env.wm.drag.forbid();
      return false;
    }
    if ((sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) {
      this.env.wm.drag.forbid();
      return false;
    }
    const pt = this.#toClient(ptWindow);
    if (!rectContains(this.#slot, pt)) return true;
    const req = this.#deps.request;
    const item = req.item(sc.id);
    if (this.#target === null && item !== null) {
      if (req.eligible(item.objId)) {
        this.#target = item;
        req.reserve(item.objId, true);
        this.#paint();
      } else {
        req.refused();
      }
    }
    return true;
  }

  /** `OnLButtonDblClk` on the slot gives the item back. */
  override onLButtonDblClk(pt: Point): void {
    if (!rectContains(this.#slot, pt) || this.#target === null) return;
    this.#deps.request.reserve(this.#target.objId, false);
    this.#target = null;
    this.#paint();
  }

  /** `OnLButtonUp`: a click on an element icon chooses it. */
  override onLButtonUp(pt: Point): void {
    const i = this.#elements.findIndex((r) => rectContains(r, pt));
    if (i < 0) return;
    this.#chosen = i;
    this.#paint();
  }

  /** The plain tooltips of `Process`. */
  override onMouseMove(pt: Point): void {
    const s = this.getScreenClientRect();
    if (rectContains(this.#slot, pt)) {
      this.#deps.tooltip.put(
        this,
        this.#deps.request.tips[0] ?? '',
        rectOffset(this.#slot, s.l, s.t),
        1,
      );
      return;
    }
    const i = this.#elements.findIndex((r) => rectContains(r, pt));
    const el = i >= 0 ? this.#elements[i] : undefined;
    if (el) {
      this.#deps.tooltip.put(
        this,
        this.#deps.request.tips[i + 1] ?? '',
        rectOffset(el, s.l, s.t),
        1,
      );
    }
  }

  // ---- OK -----------------------------------------------------------------------------------------------------------------------

  /** OK: sends `ChangeAttribute` when the item, the scroll and an element are there; the window closes either way. */
  ok(): void {
    const req = this.#deps.request;
    const t = this.#target;
    if (t !== null && this.#chosen > -1 && this.#chosen < 5) {
      req.send(t.objId, req.scroll.objId, this.#chosen + 1);
    }
    if (t !== null) req.reserve(t.objId, false);
    this.#target = null;
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.ok();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** The destructor: the reservations of the target and of the scroll go. */
  override onDestroy(): void {
    this.#release();
    super.onDestroy();
  }

  #release(): void {
    if (this.#released) return;
    this.#released = true;
    const req = this.#deps.request;
    if (this.#target) req.reserve(this.#target.objId, false);
    req.reserve(req.scroll.objId, false);
  }

  // ---- the pixels ---------------------------------------------------------------------------------------------------------------

  #toClient(ptWindow: Point): Point {
    return {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
  }

  #paint(): void {
    const t = this.#target;
    this.#items?.setLayers(
      t
        ? [
            {
              key: 'target',
              x: this.#slot.l,
              y: this.#slot.t,
              size: 32,
              icon: t.icon,
              alpha: 1,
              tint: false,
            },
          ]
        : [],
      true,
    );
    const icons = this.#deps.request.elementIcons;
    const size = ELEMENT_SPRITE * ELEMENT_ZOOM;
    const sprites: SpriteEntry[] = this.#elements.map((r, i) => ({
      key: `element${String(i)}`,
      path: icons.path,
      x: r.l,
      y: r.t,
      w: size,
      h: size,
      src: icons.cut(i),
      scale: ELEMENT_ZOOM,
    }));
    this.#sprites?.setEntries(sprites);
    const glow: RectEntry[] = [];
    const chosen = this.#elements[this.#chosen];
    if (chosen) {
      glow.push({
        key: 'chosen',
        x: chosen.l,
        y: chosen.t,
        w: chosen.r - chosen.l,
        h: chosen.b - chosen.t,
        glow: { color: argbCss(GLOW_COLOR), size: GLOW_THICKNESS },
      });
    }
    this.#rects?.setEntries(glow);
  }
}
