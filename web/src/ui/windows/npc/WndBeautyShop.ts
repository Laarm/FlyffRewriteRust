// `WndBeautyShop` (`APP_BEAUTY_SHOP_EX`, 464 x 464, Plan/45 §17; WP-UI-35 partial, WP-CFEAT-19): the hair shop. The window edits a DRAFT
// the feature keeps (style 0..9, colour red / green / blue 0..255) and shows its price; it decides nothing:
//   * `WIDC_HAIRSTYLE_LEFT` / `RIGHT` step the style (wrapping), a click on one of the four style cards (`WIDC_CUSTOM1..4`) picks it;
//   * the three colour sliders (`WIDC_CUSTOM_R/G/B`) and the three number edits (`WIDC_EDIT1..3`) set a channel (an edit is clamped
//     to 0..255 and re-synced; a slider move refreshes its edit);
//   * `WIDC_STATIC1` shows the price of the style change, `WIDC_STATIC2` of the colour change and `WIDC_COST` the total (thousands
//     separators), all 0 in coupon mode; `WIDC_BUTTON1` resets to the current look; OK asks the feature to confirm, Cancel closes;
//   * `WIDC_CUSTOM5` / `WIDC_CUSTOM6` are the before / after previews and `WIDC_CUSTOM1..4` the head of each style card: 3-D previews of the
//     own mover (doc 41 presets `beautyHair` and `hairKind`, `previews.port`); the draft's model turns (`RotationY( g_tmCurrent / 1000 )`) and
//     a selected card gets a translucent yellow fill above its preview. Without previews the cards show a swatch of the colour;
//   * in coupon mode the title carries `TID_GAME_NOWUSING_COUPON`; the window opens next to the inventory.
// Destroying the window in any way calls `actions.closed()` once (the draft is dropped).

import {
  APP_BEAUTY_SHOP_EX,
  WIDC_BUTTON1,
  WIDC_CANCEL,
  WIDC_COST,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
  WIDC_CUSTOM_B,
  WIDC_CUSTOM_G,
  WIDC_CUSTOM_R,
  WIDC_EDIT1,
  WIDC_EDIT2,
  WIDC_EDIT3,
  WIDC_HAIRSTYLE_LEFT,
  WIDC_HAIRSTYLE_RIGHT,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_STATIC2,
} from '../../../gen/defs/resdata';
import { TID_GAME_NOWUSING_COUPON } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, WSS_MONEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PreviewHandleLike } from '../login/ports';
import { placeBesideInventory, turntableDegrees } from './shopLayout';
import { BeautyCard } from './BeautyCard';
import { ChannelSlider } from './ChannelSlider';
import type {
  BeautyPreviews,
  HairPreviewSubjects,
  HairShopActions,
  HairShopView,
  ViewSource,
} from './ports';

export interface HairShopDeps {
  readonly view: ViewSource<HairShopView>;
  readonly actions: HairShopActions;
  readonly getText: (tid: number) => string;
  /** The 3-D previews (`null` / absent: the placeholder cards). */
  readonly previews?: BeautyPreviews<HairPreviewSubjects> | null;
}

const CARD_IDS = [WIDC_CUSTOM1, WIDC_CUSTOM2, WIDC_CUSTOM3, WIDC_CUSTOM4] as const;
const SLIDER_IDS = [WIDC_CUSTOM_R, WIDC_CUSTOM_G, WIDC_CUSTOM_B] as const;
const EDIT_IDS = [WIDC_EDIT1, WIDC_EDIT2, WIDC_EDIT3] as const;
const SLIDER_COLORS = ['#e04040', '#40b040', '#4060e0'] as const;

/** `0xRRGGBB` of a draft colour. */
export const rgbNumber = (rgb: readonly [number, number, number]): number =>
  ((rgb[0] & 255) << 16) | ((rgb[1] & 255) << 8) | (rgb[2] & 255);

export class WndBeautyShop extends WndNeuz {
  readonly appId = APP_BEAUTY_SHOP_EX;
  readonly #deps: HairShopDeps;
  readonly #cards: BeautyCard[] = [];
  readonly #sliders: ChannelSlider[] = [];
  readonly #edits: EditCtrl[] = [];
  #before: BeautyCard | null = null;
  #after: BeautyCard | null = null;
  #style: Static | null = null;
  #color: Static | null = null;
  #total: Static | null = null;
  #view: HairShopView | null = null;
  #off: (() => void) | null = null;
  #closedNotified = false;
  #baseTitle = '';
  #title = '';
  readonly #cardHandles: (PreviewHandleLike | null)[] = [];
  readonly #cardMesh: (number | undefined)[] = [];
  #beforeHandle: PreviewHandleLike | null = null;
  #afterHandle: PreviewHandleLike | null = null;
  #beforeSig = '';
  #afterSig = '';

  constructor(deps: HairShopDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get cards(): readonly BeautyCard[] {
    return this.#cards;
  }
  get sliders(): readonly ChannelSlider[] {
    return this.#sliders;
  }
  get edits(): readonly EditCtrl[] {
    return this.#edits;
  }
  get beforeCard(): BeautyCard | null {
    return this.#before;
  }
  get afterCard(): BeautyCard | null {
    return this.#after;
  }
  get stylePrice(): Static | null {
    return this.#style;
  }
  get colorPrice(): Static | null {
    return this.#color;
  }
  get totalPrice(): Static | null {
    return this.#total;
  }
  /** The open previews: before, after and the four cards (empty without a port). */
  get previewHandles(): readonly PreviewHandleLike[] {
    return [this.#beforeHandle, this.#afterHandle, ...this.#cardHandles].filter(
      (h): h is PreviewHandleLike => h !== null,
    );
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get resetButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#baseTitle = this.getTitle();
    this.#title = this.#baseTitle;
    this.#style = this.getDlgItem<Static>(WIDC_STATIC1);
    this.#color = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#total = this.getDlgItem<Static>(WIDC_COST);
    for (const s of [this.#style, this.#color, this.#total]) s?.addWndStyle(WSS_MONEY);
    for (const [i, id] of CARD_IDS.entries()) {
      const rect = this.getWndCtrl(id)?.rect;
      if (!rect) continue;
      const card = this.addControl(new BeautyCard(), id, rect);
      card.setPickable(true);
      card.onPick = () => {
        const mesh = this.#view?.cards[i];
        if (mesh !== undefined) this.#deps.actions.pick(mesh);
      };
      this.#cards.push(card);
    }
    const before = this.getWndCtrl(WIDC_CUSTOM5)?.rect;
    if (before) this.#before = this.addControl(new BeautyCard(), WIDC_CUSTOM5, before);
    const after = this.getWndCtrl(WIDC_CUSTOM6)?.rect;
    if (after) this.#after = this.addControl(new BeautyCard(), WIDC_CUSTOM6, after);
    for (const [i, id] of SLIDER_IDS.entries()) {
      const rect = this.getWndCtrl(id)?.rect;
      if (!rect) continue;
      const slider = this.addControl(new ChannelSlider(SLIDER_COLORS[i] ?? '#808080'), id, rect);
      slider.onChange = (v) => {
        this.#edits[i]?.setString(String(v));
        this.#deps.actions.setChannel(i as 0 | 1 | 2, v);
      };
      this.#sliders.push(slider);
    }
    for (const id of EDIT_IDS) {
      const edit = this.getDlgItem<EditCtrl>(id);
      if (!edit) continue;
      edit.addWndStyle(EBS_NUMBER);
      this.#edits.push(edit);
    }
    this.getDlgItem<Button>(WIDC_OK)?.setDefault(true);
    placeBesideInventory(this);
    this.#openPreviews();
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  /** The previews of the original: the player as is, the draft turning, and the head of each style (`OnDraw`, `DrawHairKind`). */
  #openPreviews(): void {
    const pv = this.#deps.previews;
    if (!pv) return;
    const v = this.#deps.view.get();
    const open = (id: number, preset: 'beautyHair' | 'hairKind', subject: unknown) => {
      const canvas = this.customCanvas(id);
      return canvas ? pv.port.open({ canvas, preset, subject, animate: false }) : null;
    };
    this.#beforeHandle = open(WIDC_CUSTOM5, 'beautyHair', pv.subjects.before());
    this.#beforeSig = this.#sigBefore(v);
    this.#afterHandle = open(WIDC_CUSTOM6, 'beautyHair', pv.subjects.after(v));
    this.#afterSig = this.#sigAfter(v);
    for (const [i, id] of CARD_IDS.entries()) {
      const mesh = v.cards[i];
      this.#cardMesh[i] = mesh;
      this.#cardHandles.push(
        mesh === undefined ? null : open(id, 'hairKind', pv.subjects.card(mesh)),
      );
    }
    for (const [i, card] of this.#cards.entries()) card.setOverlay(this.#cardHandles[i] != null);
    this.#before?.setOverlay(this.#beforeHandle !== null);
    this.#after?.setOverlay(this.#afterHandle !== null);
  }

  #sigBefore = (v: HairShopView): string => `${String(v.currentMesh)}:${v.currentRgb.join(',')}`;
  #sigAfter = (v: HairShopView): string => `${String(v.mesh)}:${v.rgb.join(',')}`;

  /** The previews follow the view: a new draft re-assembles the right model, a new card page the head cards. */
  #syncPreviews(v: HairShopView): void {
    const pv = this.#deps.previews;
    if (!pv) return;
    if (this.#beforeHandle && this.#sigBefore(v) !== this.#beforeSig) {
      this.#beforeSig = this.#sigBefore(v);
      this.#beforeHandle.setSubject(pv.subjects.before());
    }
    if (this.#afterHandle && this.#sigAfter(v) !== this.#afterSig) {
      this.#afterSig = this.#sigAfter(v);
      this.#afterHandle.setSubject(pv.subjects.after(v));
    }
    for (const [i, handle] of this.#cardHandles.entries()) {
      const mesh = v.cards[i];
      if (!handle || mesh === undefined || mesh === this.#cardMesh[i]) continue;
      this.#cardMesh[i] = mesh;
      handle.setSubject(pv.subjects.card(mesh));
    }
  }

  #apply(v: HairShopView): void {
    this.#view = v;
    for (const [i, card] of this.#cards.entries()) {
      const mesh = v.cards[i];
      if (mesh === undefined) {
        card.set('', null, false);
        continue;
      }
      card.set(String(mesh + 1), rgbNumber(v.rgb), mesh === v.mesh);
    }
    this.#before?.set('Before', rgbNumber(v.currentRgb), false);
    this.#after?.set('After', rgbNumber(v.rgb), false);
    for (const [i, slider] of this.#sliders.entries()) slider.setValue(v.rgb[i] ?? 0);
    for (const [i, edit] of this.#edits.entries()) {
      const text = String(v.rgb[i] ?? 0);
      if (edit.getString() !== text) edit.setString(text);
    }
    const title = v.coupon
      ? `${this.#baseTitle}  ${this.#deps.getText(TID_GAME_NOWUSING_COUPON)}`
      : this.#baseTitle;
    if (title !== this.#title) {
      this.#title = title;
      this.setTitle(title);
    }
    this.#syncPreviews(v);
    this.#style?.setTitle(String(v.coupon ? 0 : v.stylePrice));
    this.#color?.setTitle(String(v.coupon ? 0 : v.colorPrice));
    this.#total?.setTitle(String(v.coupon ? 0 : v.total));
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#view) this.#apply(v);
    this.#afterHandle?.setRotation?.(turntableDegrees(now));
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const a = this.#deps.actions;
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_HAIRSTYLE_LEFT) {
        a.step(-1);
        return true;
      }
      if (id === WIDC_HAIRSTYLE_RIGHT) {
        a.step(1);
        return true;
      }
      if (id === WIDC_BUTTON1) {
        a.reset();
        return true;
      }
      if (id === WIDC_OK) {
        a.ok();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    if (msg === Wnm.EN_CHANGE) {
      const i = EDIT_IDS.indexOf(id as (typeof EDIT_IDS)[number]);
      if (i >= 0) {
        const edit = this.#edits[i];
        const n = Math.min(255, Math.max(0, Number.parseInt(edit?.getString() ?? '', 10) || 0));
        a.setChannel(i as 0 | 1 | 2, n);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    for (const h of [this.#beforeHandle, this.#afterHandle, ...this.#cardHandles]) h?.close();
    this.#beforeHandle = null;
    this.#afterHandle = null;
    this.#cardHandles.length = 0;
    if (!this.#closedNotified) {
      this.#closedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}
