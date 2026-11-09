// `WndFindWordGame` (`CWndFindWordGame`, `src:_Interface/WndMiniGame.cpp:613-905`, ref/22 §17 P; WP-UI-63): `APP_MINIGAME_WORD` (612, 288 x 256, "Minigame (Find Word)"), the letter game. Five slots
// (`WORD_NUM1..5`, 32 x 32) take the cards `A` .. `Z` from the bag (a drop on the slot under the pointer; a D3 double click of a card in the bag goes to the first empty slot, the feature's
// `registerFeed`); a double click on a filled slot gives the card back. The hint letter the server named is drawn dimmed (alpha 100) in its slot. `TEXT_DESC` shows the panel
// `minigamefindworddesc` and, once the server asked the question, the question. Start (`BTN_START`) is disabled until all five slots are filled. The rules are the machine's
// (`features/events/minigame/findWord.ts`); opening sends `AlphabetOpenWnd` (the request's `opened`). Esc closes the window (it is not in `ESC_KEEP_APPS`).

import { APP_MINIGAME_WORD, WIDC_BTN_START, WIDC_TEXT_DESC } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { Point, Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { clientPoint, slotAt } from './cardSlots';
import type { FindWordRequest } from './ports';

const ID_SPRITES = 0x7b30;
/** The card icons are 32 x 32; the hint is drawn at `Render( ..., 100 )`. */
const CARD_SIZE = 32;
const HINT_ALPHA = 100 / 255;
/** The five slots are `WORD_NUM1 .. WORD_NUM5` (ids 929, 900, 936, 867, 872 in layout order). */
const SLOT_IDS = [929, 900, 936, 867, 872] as const;

export class WndFindWordGame extends WndNeuz {
  readonly appId = APP_MINIGAME_WORD;
  override readonly processEveryFrame = true;
  readonly #request: FindWordRequest;
  #rects: Rect[] = [];
  #sprites: SpriteLayer | null = null;
  #description = '';

  constructor(request: FindWordRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_START);
  }
  get descriptionText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT_DESC);
  }
  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  /** The layout rectangle of a slot (client coordinates; tests). */
  slotRect(slot: number): Rect | undefined {
    return this.#rects[slot];
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#rects = SLOT_IDS.map((id) => this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 });
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, this.getClientRect(false));
    this.startButton?.enableWindow(false);
    this.#request.opened?.();
    this.#sync();
    this.moveParentCenter();
  }

  #sync(): void {
    const v = this.#request.view();
    this.startButton?.enableWindow(v.startEnabled);
    if (v.description !== this.#description) {
      this.#description = v.description;
      this.descriptionText?.setParsingString(v.description);
    }
    const entries: SpriteEntry[] = [];
    if (v.hint?.card.icon) {
      const r = this.#rects[v.hint.slot];
      if (r) {
        entries.push({
          key: 'hint',
          path: v.hint.card.icon,
          x: r.l,
          y: r.t,
          w: CARD_SIZE,
          h: CARD_SIZE,
          alpha: HINT_ALPHA,
        });
      }
    }
    v.slots.forEach((card, i) => {
      const r = this.#rects[i];
      if (!card?.icon || !r) return;
      entries.push({
        key: `card${String(i)}`,
        path: card.icon,
        x: r.l,
        y: r.t,
        w: CARD_SIZE,
        h: CARD_SIZE,
      });
    });
    this.#sprites?.setEntries(entries);
  }

  override process(now: number): void {
    this.#sync();
    super.process(now);
  }

  /** `OnDropIcon`: a card of the bag on the slot under the pointer. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM) {
      this.env.wm.drag.forbid();
      return false;
    }
    const slot = slotAt(this.#rects, clientPoint(ptWindow, this.rectClient, this.rectWindow));
    this.#request.drop(slot, sc.id);
    this.#sync();
    return true;
  }

  /** `OnLButtonDblClk`: the card in the slot goes back. */
  override onLButtonDblClk(pt: Point): void {
    const slot = slotAt(this.#rects, pt);
    if (slot >= 0) {
      this.#request.empty(slot);
      this.#sync();
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BTN_START) {
      this.#request.start();
      this.#sync();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed();
    super.onDestroy();
  }
}
