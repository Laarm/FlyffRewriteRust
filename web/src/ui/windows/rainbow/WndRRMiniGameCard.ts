// `WndRRMiniGameCard` (`CWndRRMiniGameCard`, `src:_Interface/WndRainbowRace.cpp:1942-2151`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-61): `APP_RR_MINIGAME_CARD` (800, 496 x 432), "FlyFF Concentration". 18 cards of 70 x 80
// in 6 x 3 cells (`CUSTOM1-18`): `setDeck( s )` names the cards (card `i` is byte `i` minus one, kinds 0..8 = `Theme/RainbowRaceCard<n+1>.tga`), all face down (`RainbowRaceCardBack.tga`); a card shows its face when
// it is matched or picked. A click (button up) on a cell: the first pick is only remembered, the second (a different cell) sends the pair; a result clears both picks (TRUE keeps the pair face up, KEEP: a
// mismatched pair flips back when the answer arrives). After a mismatch the clicks are blocked for 5 000 ms (blocked at 4 999, allowed at 5 001) and the digit `4 - floor( elapsed / 1000 )` of
// `theme/rainbowcardpenaltynumber.inc` stands at (180, 50). FIX: a click on a matched card is ignored. The select cursor shows over a face-down card while no penalty runs.

import {
  APP_RR_MINIGAME_CARD,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
  WIDC_CUSTOM7,
  WIDC_CUSTOM8,
  WIDC_CUSTOM9,
  WIDC_CUSTOM10,
  WIDC_CUSTOM11,
  WIDC_CUSTOM12,
  WIDC_CUSTOM13,
  WIDC_CUSTOM14,
  WIDC_CUSTOM15,
  WIDC_CUSTOM16,
  WIDC_CUSTOM17,
  WIDC_CUSTOM18,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { type Point, type Rect } from '../../layout/geometry';
import type { UiAtlases } from '../../skin/atlas';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CardModel, CARD_COUNT } from './card';
import { CARD_BACK, cardPicture, MP, PENALTY_NUMBER_ATLAS } from './constants';
import type { CardRequest } from './ports';

const CELLS = [
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
  WIDC_CUSTOM7,
  WIDC_CUSTOM8,
  WIDC_CUSTOM9,
  WIDC_CUSTOM10,
  WIDC_CUSTOM11,
  WIDC_CUSTOM12,
  WIDC_CUSTOM13,
  WIDC_CUSTOM14,
  WIDC_CUSTOM15,
  WIDC_CUSTOM16,
  WIDC_CUSTOM17,
  WIDC_CUSTOM18,
] as const;
const ID_CARDS = 0x5430;
/** The penalty digit's position. */
export const PENALTY_AT = { x: 180, y: 50 } as const;
const CUR_SELECT = 10;

export interface CardDeps {
  readonly atlases: UiAtlases;
}

export class WndRRMiniGameCard extends WndNeuz {
  readonly appId = APP_RR_MINIGAME_CARD;
  readonly #request: CardRequest;
  readonly #deps: CardDeps;
  readonly cards = new CardModel();
  #cells: Rect[] = [];
  #layer: SpriteLayer | null = null;
  #pointer: Point | null = null;

  constructor(request: CardRequest, deps: CardDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get cardLayer(): SpriteLayer | null {
    return this.#layer;
  }
  /** The cell rects of the layout (`CUSTOM1-18`). */
  get cellRects(): readonly Rect[] {
    return this.#cells;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<TextCtrl>(WIDC_TEXT1)?.setString(this.#request.text);
    this.#cells = CELLS.map((id) => this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 });
    this.#layer = this.addControl(new SpriteLayer(), ID_CARDS, this.getClientRect(false));
    if (this.#request.deck !== undefined) this.cards.setDeck(this.#request.deck);
    this.#paint();
    this.moveParentCenter();
  }

  /** `SetQuestion`. */
  setDeck(deck: string): void {
    this.cards.setDeck(deck);
    this.#paint();
  }

  /** `ReceiveResult( state )`: TRUE / FALSE / FAIL of a pair. */
  result(state: number): void {
    this.cards.result(state, this.#request.now(), MP.TRUE, MP.FALSE);
    this.#paint();
  }

  #paint(): void {
    const entries: SpriteEntry[] = [];
    for (let i = 0; i < CARD_COUNT; i += 1) {
      const r = this.#cells[i];
      if (!r) continue;
      const kind = this.cards.kinds[i] ?? -1;
      const face = this.cards.faceUp(i) && kind >= 0;
      entries.push({
        key: `card${String(i)}`,
        path: face ? cardPicture(kind) : CARD_BACK,
        x: r.l,
        y: r.t,
        w: 70,
        h: 80,
      });
    }
    const digit = this.cards.penaltyDigit(this.#request.now());
    const dto = this.#deps.atlases.info(PENALTY_NUMBER_ATLAS);
    const cut = digit === null ? undefined : dto?.sprites[digit];
    if (dto && cut) {
      entries.push({
        key: 'penalty',
        path: dto.texture,
        x: PENALTY_AT.x,
        y: PENALTY_AT.y,
        w: cut.w,
        h: cut.h,
        src: { x: cut.x, y: cut.y },
      });
    }
    this.#layer?.setEntries(entries);
  }

  /** The cell under a client point, -1 for none. */
  cellAt(pt: Point): number {
    return this.#cells.findIndex((r) => pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b);
  }

  override onLButtonUp(pt: Point): void {
    const i = this.cellAt(pt);
    if (i < 0) return;
    const pair = this.cards.click(i, this.#request.now());
    this.#paint();
    if (pair) this.#request.pick(pair[0], pair[1]);
  }

  override onMouseMove(pt: Point): void {
    this.#pointer = pt;
  }
  override onMouseLeave(): void {
    this.#pointer = null;
  }

  /** `OnSetCursor`: the select cursor over a face-down card while no penalty runs. */
  uiCursor(): number | null {
    if (this.#pointer === null) return null;
    const i = this.cellAt(this.#pointer);
    return i >= 0 && this.cards.clickable(i, this.#request.now()) ? CUR_SELECT : null;
  }

  /** The penalty digit follows the clock. */
  override process(now: number): void {
    super.process(now);
    this.#paint();
  }
}
