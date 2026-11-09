// `WndPuzzleGame` (`CWndPuzzleGame`, `src:_Interface/WndMiniGame.cpp:1598-1950`, ref/22 §17 P; WP-UI-63): `APP_MINIGAME_PUZZLE` (614, 224 x 320, "Minigame (Puzzle)"), the 3 x 3 picture game. Nine slots
// (`PIC_NUM1..9`, 32 x 32 at (60, 36) .. (124, 100) + 32 steps) take the cards the server named (`REASSEMBLE_OPENWND`): a drop or a D3 double click of a card fills EVERY empty slot of that card
// up to the stack's count, a double click on a filled slot empties every slot of that card. `TEXT_DESC` shows the panel `minigamepuzzledesc`. Start (`BTN_START`) is disabled until all nine
// slots are filled. The rules are the machine's (`features/events/minigame/puzzle.ts`); opening sends `ReassembleOpenWnd` (the request's `opened`). Esc closes the window.

import { APP_MINIGAME_PUZZLE, WIDC_BTN_START, WIDC_TEXT_DESC } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { Point, Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { slotAt } from './cardSlots';
import type { PuzzleRequest } from './ports';

const ID_SPRITES = 0x7b40;
const CARD_SIZE = 32;
/** `PIC_NUM1 .. PIC_NUM9` in layout order. */
const SLOT_IDS = [900, 936, 867, 872, 886, 862, 863, 865, 866] as const;

export class WndPuzzleGame extends WndNeuz {
  readonly appId = APP_MINIGAME_PUZZLE;
  override readonly processEveryFrame = true;
  readonly #request: PuzzleRequest;
  #rects: Rect[] = [];
  #sprites: SpriteLayer | null = null;
  #description = '';

  constructor(request: PuzzleRequest) {
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

  /** `OnDropIcon`: a stack of the bag fills every slot of its card. */
  override onDropIcon(sc: Shortcut): boolean {
    if (sc.kind !== SC.ITEM) {
      this.env.wm.drag.forbid();
      return false;
    }
    this.#request.drop(sc.id);
    this.#sync();
    return true;
  }

  /** `OnLButtonDblClk`: the card in the slot (and every other slot of it) goes back. */
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
