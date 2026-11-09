// `WndHeroSkillUp` (`CWndHeroSkillUp`, `src:_Interface/WndField.cpp:21611-21915`, ref/22 §17 D, Plan/42 §19.4 D, 45 §19.2; WP-UI-38):
// `APP_HERO_SKILLUP` "Hero Skill Level" (304 x 288), opened by the legend NPC (`MMI_LEGEND_SKILLUP`). Five gem slots in a row, each
// takes exactly one kind (diamond, emerald, sapphire, ruby, topaz) by drop or by the double click of the inventory; a double click on
// a slot takes the gem back. The default gem pictures sit above the slots; OK is enabled when all five slots are filled and sends
// `LegendSkillUpStart` once; the answer (`LegendSkillUpResult`) shows a message and closes the window. The window reserves the gems
// it holds (`SetExtra(+1)`, released again when a slot is emptied or the window closes).

import { APP_HERO_SKILLUP } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, rectContains } from '../../layout/geometry';
import type { WidcKey } from '../../layout/registry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { HeroSkillUpRequest } from './ports';

/** The window id of the sprite layer (a code-created control). */
const LAYER_ID = 0x7c11;
const SLOTS = 5;
const PIC: readonly WidcKey[] = [
  'WIDC_PIC_SLOT1',
  'WIDC_PIC_SLOT2',
  'WIDC_PIC_SLOT3',
  'WIDC_PIC_SLOT4',
  'WIDC_PIC_SLOT5',
];
const DEFAULT: readonly WidcKey[] = [
  'WIDC_CUSTOM1',
  'WIDC_CUSTOM2',
  'WIDC_CUSTOM3',
  'WIDC_CUSTOM4',
  'WIDC_CUSTOM5',
];

export class WndHeroSkillUp extends WndNeuz {
  readonly appId = APP_HERO_SKILLUP;
  readonly #request: HeroSkillUpRequest;
  /** `m_pItemElem[5]` as the inventory object ids, `null` = empty. */
  #slots: (number | null)[] = [null, null, null, null, null];
  /** `m_bSendHeroSkillup`. */
  #sent = false;
  #layer: SpriteLayer | null = null;

  constructor(request: HeroSkillUpRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** The object ids in the slots (`null` = empty). */
  get slots(): readonly (number | null)[] {
    return this.#slots;
  }
  get sent(): boolean {
    return this.#sent;
  }
  get layer(): SpriteLayer | null {
    return this.#layer;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>('WIDC_OK');
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>('WIDC_CANCEL');
  }
  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>('WIDC_TEXT1');
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.okButton?.enableWindow(false);
    if (this.#request.description !== '') this.text?.setString(this.#request.description);
    this.#layer = this.addControl(new SpriteLayer(), LAYER_ID, this.getClientRect(false));
    this.#refresh();
    this.moveParentCenter();
  }

  /** `HitTest`: the slot under a client point, `-1` = none. */
  hitTest(pt: Point): number {
    for (let i = 0; i < SLOTS; i += 1) {
      const name = PIC[i];
      const r = name ? this.getWndCtrl(name)?.rect : undefined;
      if (r && rectContains(r, pt)) return i;
    }
    return -1;
  }

  /** `SetJewel` (the double click of the inventory): the gem goes to the slot of its kind when that is empty. */
  setJewel(objId: number): boolean {
    const itemId = this.#request.itemId(objId);
    if (itemId === undefined) return false;
    const slot = this.#request.jewelIds.indexOf(itemId);
    return this.#put(slot, objId);
  }

  #put(slot: number, objId: number): boolean {
    if (slot < 0 || this.#slots[slot] !== null) return false;
    if (this.#request.itemId(objId) !== this.#request.jewelIds[slot]) return false;
    this.#slots[slot] = objId;
    this.#request.reserve(objId, true);
    this.#refresh();
    return true;
  }

  /** `OnDropIcon`: a gem of the right kind dropped on an empty slot. */
  override onDropIcon(sc: Shortcut, pt: Point): boolean {
    const slot = this.hitTest(pt);
    if (slot >= 0 && sc.kind === SC.ITEM) this.#put(slot, sc.id);
    return true;
  }

  /** `OnLButtonDblClk`: a double click on a filled slot takes the gem out. */
  override onLButtonDblClk(pt: Point): void {
    const slot = this.hitTest(pt);
    const obj = slot < 0 ? null : this.#slots[slot];
    if (obj === null || obj === undefined) return;
    this.#request.reserve(obj, false);
    this.#slots[slot] = null;
    this.#refresh();
  }

  /** `ReceiveResult`: -1 the skill is at its maximum, 0 failed, 1 succeeded; the window closes. */
  receiveResult(result: number): void {
    this.#request.result?.(result);
    this.destroy();
  }

  #refresh(): void {
    const filled = this.#slots.every((s) => s !== null);
    this.okButton?.enableWindow(filled);
    const entries: SpriteEntry[] = [];
    for (let i = 0; i < SLOTS; i += 1) {
      const dn = DEFAULT[i];
      const d = dn ? this.getWndCtrl(dn)?.rect : undefined;
      const di = this.#request.jewelIcons[i] ?? null;
      if (d && di)
        entries.push({ key: `default:${String(i)}`, path: di, x: d.l, y: d.t, w: 32, h: 32 });
      const obj = this.#slots[i] ?? null;
      const pn = PIC[i];
      const p = pn ? this.getWndCtrl(pn)?.rect : undefined;
      if (obj !== null && p) {
        const icon = this.#request.itemIcon(obj);
        if (icon)
          entries.push({ key: `slot:${String(i)}`, path: icon, x: p.l, y: p.t, w: 32, h: 32 });
      }
    }
    this.#layer?.setEntries(entries);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === this.okButton?.id) {
        const ids = this.#slots.flatMap((s) => (s === null ? [] : [s]));
        if (!this.#sent && ids.length === SLOTS) {
          this.#sent = true;
          this.#request.start(ids);
        }
        return true;
      }
      if (id === this.cancelButton?.id) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    for (const obj of this.#slots) if (obj !== null) this.#request.reserve(obj, false);
    this.#slots = [null, null, null, null, null];
    this.#request.closed?.();
    super.onDestroy();
  }
}
