// `WndFaceShop` (`APP_BEAUTY_SHOP_SKIN`, 464 x 464, Plan/45 §17; WP-UI-35 partial, WP-CFEAT-19): the face shop. Two rows of four face
// cards (`WIDC_CUSTOM1..4` "Friendly Face", `WIDC_CUSTOM7..10` "New Face") page through the heads with their own arrow buttons
// (`WIDC_FRIENDSHIPFACE_LEFT/RIGHT`, `WIDC_NEWFACE_LEFT/RIGHT`); a click on a card picks that face (the selected one is framed in
// both rows). `WIDC_CUSTOM5` / `6` are the before / after previews (the whole body, the draft turning) and the eight cards show the
// head of each face: 3-D previews of the own mover (doc 41 presets `beautyHair` and `faceKind`, `previews.port`); the selected card gets a
// translucent yellow fill above its preview. Reset goes back to the current face, OK asks the feature to confirm
// (`APP_BEAUTY_SHOP_EX_CONFIRM`), Cancel closes. The price is not on the window of the original; the feature prints it in the
// confirmation. The title carries `TID_GAME_NOWUSING_COUPON` in coupon mode and the window opens next to the inventory. Destroying the
// window calls `actions.closed()` once.

import {
  APP_BEAUTY_SHOP_SKIN,
  WIDC_BTN_CANCEL,
  WIDC_BTN_OK,
  WIDC_BTN_RESET,
  WIDC_CUSTOM1,
  WIDC_CUSTOM10,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
  WIDC_CUSTOM7,
  WIDC_CUSTOM8,
  WIDC_CUSTOM9,
  WIDC_FRIENDSHIPFACE_LEFT,
  WIDC_FRIENDSHIPFACE_RIGHT,
  WIDC_NEWFACE_LEFT,
  WIDC_NEWFACE_RIGHT,
} from '../../../gen/defs/resdata';
import { TID_GAME_NOWUSING_COUPON } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PreviewHandleLike } from '../login/ports';
import { placeBesideInventory, turntableDegrees } from './shopLayout';
import { BeautyCard } from './BeautyCard';
import type {
  BeautyPreviews,
  FacePreviewSubjects,
  FaceShopActions,
  FaceShopView,
  ViewSource,
} from './ports';

export interface FaceShopDeps {
  readonly view: ViewSource<FaceShopView>;
  readonly actions: FaceShopActions;
  readonly getText: (tid: number) => string;
  /** The 3-D previews (`null` / absent: the placeholder cards). */
  readonly previews?: BeautyPreviews<FacePreviewSubjects> | null;
}

const FRIENDLY_IDS = [WIDC_CUSTOM1, WIDC_CUSTOM2, WIDC_CUSTOM3, WIDC_CUSTOM4] as const;
const NEW_IDS = [WIDC_CUSTOM7, WIDC_CUSTOM8, WIDC_CUSTOM9, WIDC_CUSTOM10] as const;

export class WndFaceShop extends WndNeuz {
  readonly appId = APP_BEAUTY_SHOP_SKIN;
  readonly #deps: FaceShopDeps;
  readonly #friendly: BeautyCard[] = [];
  readonly #fresh: BeautyCard[] = [];
  #before: BeautyCard | null = null;
  #after: BeautyCard | null = null;
  #view: FaceShopView | null = null;
  #off: (() => void) | null = null;
  #closedNotified = false;
  #baseTitle = '';
  #title = '';
  #beforeHandle: PreviewHandleLike | null = null;
  #afterHandle: PreviewHandleLike | null = null;
  /** The handles of the eight cards (friendly row first) and the face each shows. */
  readonly #cardHandles: (PreviewHandleLike | null)[] = [];
  readonly #cardFace: (number | undefined)[] = [];
  #beforeFace = -1;
  #afterFace = -1;

  constructor(deps: FaceShopDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get friendlyCards(): readonly BeautyCard[] {
    return this.#friendly;
  }
  get newCards(): readonly BeautyCard[] {
    return this.#fresh;
  }
  get beforeCard(): BeautyCard | null {
    return this.#before;
  }
  get afterCard(): BeautyCard | null {
    return this.#after;
  }
  /** The open previews: before, after, the four friendly cards and the four new ones. */
  get previewHandles(): readonly PreviewHandleLike[] {
    return [this.#beforeHandle, this.#afterHandle, ...this.#cardHandles].filter(
      (h): h is PreviewHandleLike => h !== null,
    );
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_OK);
  }

  #cards(ids: readonly number[], pick: (view: FaceShopView) => readonly number[]): BeautyCard[] {
    const out: BeautyCard[] = [];
    for (const [i, id] of ids.entries()) {
      const rect = this.getWndCtrl(id)?.rect;
      if (!rect) continue;
      const card = this.addControl(new BeautyCard(), id, rect);
      card.setPickable(true);
      card.onPick = () => {
        const v = this.#view;
        const face = v ? pick(v)[i] : undefined;
        if (face !== undefined) this.#deps.actions.pick(face);
      };
      out.push(card);
    }
    return out;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#baseTitle = this.getTitle();
    this.#title = this.#baseTitle;
    this.#friendly.push(...this.#cards(FRIENDLY_IDS, (v) => v.friendly));
    this.#fresh.push(...this.#cards(NEW_IDS, (v) => v.fresh));
    const before = this.getWndCtrl(WIDC_CUSTOM5)?.rect;
    if (before) this.#before = this.addControl(new BeautyCard(), WIDC_CUSTOM5, before);
    const after = this.getWndCtrl(WIDC_CUSTOM6)?.rect;
    if (after) this.#after = this.addControl(new BeautyCard(), WIDC_CUSTOM6, after);
    this.getDlgItem<Button>(WIDC_BTN_OK)?.setDefault(true);
    placeBesideInventory(this);
    this.#openPreviews();
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  /** The previews of the original (`OnDraw`, `DrawFaces`): the player as is, the draft turning, the head of each face. */
  #openPreviews(): void {
    const pv = this.#deps.previews;
    if (!pv) return;
    const v = this.#deps.view.get();
    const open = (id: number, preset: 'beautyHair' | 'faceKind', subject: unknown) => {
      const canvas = this.customCanvas(id);
      return canvas ? pv.port.open({ canvas, preset, subject, animate: false }) : null;
    };
    this.#beforeHandle = open(WIDC_CUSTOM5, 'beautyHair', pv.subjects.before());
    this.#beforeFace = v.currentFace;
    this.#afterHandle = open(WIDC_CUSTOM6, 'beautyHair', pv.subjects.after(v));
    this.#afterFace = v.face;
    const faces = [...v.friendly, ...v.fresh];
    [...FRIENDLY_IDS, ...NEW_IDS].forEach((id, i) => {
      const face = faces[i];
      this.#cardFace[i] = face;
      this.#cardHandles.push(
        face === undefined ? null : open(id, 'faceKind', pv.subjects.card(face)),
      );
    });
    [...this.#friendly, ...this.#fresh].forEach((card, i) => {
      card.setOverlay(this.#cardHandles[i] != null);
    });
    this.#before?.setOverlay(this.#beforeHandle !== null);
    this.#after?.setOverlay(this.#afterHandle !== null);
  }

  #syncPreviews(v: FaceShopView): void {
    const pv = this.#deps.previews;
    if (!pv) return;
    if (this.#beforeHandle && v.currentFace !== this.#beforeFace) {
      this.#beforeFace = v.currentFace;
      this.#beforeHandle.setSubject(pv.subjects.before());
    }
    if (this.#afterHandle && v.face !== this.#afterFace) {
      this.#afterFace = v.face;
      this.#afterHandle.setSubject(pv.subjects.after(v));
    }
    const faces = [...v.friendly, ...v.fresh];
    for (const [i, handle] of this.#cardHandles.entries()) {
      const face = faces[i];
      if (!handle || face === undefined || face === this.#cardFace[i]) continue;
      this.#cardFace[i] = face;
      handle.setSubject(pv.subjects.card(face));
    }
  }

  #apply(v: FaceShopView): void {
    this.#view = v;
    const fill = (cards: readonly BeautyCard[], faces: readonly number[]): void => {
      for (const [i, card] of cards.entries()) {
        const face = faces[i];
        if (face === undefined) card.set('', null, false);
        else card.set(`Face ${String(face + 1)}`, null, face === v.face);
      }
    };
    fill(this.#friendly, v.friendly);
    fill(this.#fresh, v.fresh);
    this.#before?.set(`Face ${String(v.currentFace + 1)}`, null, false);
    this.#after?.set(`Face ${String(v.face + 1)}`, null, false);
    const title = v.coupon
      ? `${this.#baseTitle}  ${this.#deps.getText(TID_GAME_NOWUSING_COUPON)}`
      : this.#baseTitle;
    if (title !== this.#title) {
      this.#title = title;
      this.setTitle(title);
    }
    this.#syncPreviews(v);
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
      switch (id) {
        case WIDC_FRIENDSHIPFACE_LEFT:
          a.stepFriendly(-1);
          return true;
        case WIDC_FRIENDSHIPFACE_RIGHT:
          a.stepFriendly(1);
          return true;
        case WIDC_NEWFACE_LEFT:
          a.stepNew(-1);
          return true;
        case WIDC_NEWFACE_RIGHT:
          a.stepNew(1);
          return true;
        case WIDC_BTN_RESET:
          a.reset();
          return true;
        case WIDC_BTN_OK:
          a.ok();
          return true;
        case WIDC_BTN_CANCEL:
          this.destroy();
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
