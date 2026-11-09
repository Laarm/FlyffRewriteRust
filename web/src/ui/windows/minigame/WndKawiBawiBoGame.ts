// `WndKawiBawiBoGame` (`CWndKawiBawiBoGame`, `src:_Interface/WndMiniGame.cpp:17-318`, ref/22 §17 P, Plan/42 §19.4 D, 45 §28.0; WP-UI-63): `APP_MINIGAME_KAWIBAWIBO` (619, 288 x 288,
// "Rock-Paper-Scissors"), the rock-paper-scissors game of the NPC "Gawibawibo". The window draws the own hand and the NPC's hand (`Icon_<hand>1.dds` / `Icon_<hand>2.dds`, 90 x 90, at
// `STATIC_MY` / `STATIC_COM` + (3, 8)), the two captions `"        " + TID_GAME_KAWIBAWIBO_KAWI | BAWI | BO` (eight blanks for the icon) and the straight wins; the buttons cycle the own hand
// (`BTN_LEFT` / `BTN_RIGHT`) and start a round (`BTN_START`). The rules - the spin of the NPC's hand, the result, the coupon check, which buttons are enabled - are the machine's
// (`features/events/minigame/kawibawibo.ts`); the window counts 60 Hz frames for it (`FrameClock`), mirrors its view and forwards the clicks. The title CLOSE button is refused while the
// machine says so (`m_nWinningCount > 0 || m_nStatus != 0 || m_nPrevResult == DRAW`: the player holds a streak or a draw); Esc keeps the window (`ESC_KEEP_APPS`).

import {
  APP_MINIGAME_KAWIBAWIBO,
  WIDC_BTN_LEFT,
  WIDC_BTN_RIGHT,
  WIDC_BTN_START,
  WIDC_COM_CHOICE,
  WIDC_MY_CHOICE,
  WIDC_STATIC_COM,
  WIDC_STATIC_MY,
  WIDC_WINNING_COUNT,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_KAWIBAWIBO_BAWI,
  TID_GAME_KAWIBAWIBO_BO,
  TID_GAME_KAWIBAWIBO_KAWI,
  TID_GAME_KWAIBAWIBO_WINCOUNT,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import type { Rect } from '../../layout/geometry';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { FrameClock } from './frames';
import {
  HAND_NAMES,
  type KawibawiboRequest,
  type KawibawiboView,
  type MiniGameHost,
} from './ports';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_SPRITES = 0x7b10;
/** The hands are drawn at the top-left of their statics + (3, 8). */
const HAND_AT = { x: 3, y: 8 } as const;
const HAND_SIZE = 90;
/** The caption of a hand starts eight blanks in (the icon sits to its left). */
const CAPTION_INDENT = '        ';
const HAND_TIDS = [
  TID_GAME_KAWIBAWIBO_KAWI,
  TID_GAME_KAWIBAWIBO_BAWI,
  TID_GAME_KAWIBAWIBO_BO,
] as const;

export class WndKawiBawiBoGame extends WndNeuz {
  readonly appId = APP_MINIGAME_KAWIBAWIBO;
  override readonly processEveryFrame = true;
  readonly #request: KawibawiboRequest;
  readonly #host: MiniGameHost;
  readonly #clock = new FrameClock();
  #sprites: SpriteLayer | null = null;
  #my: Rect = NO_RECT;
  #com: Rect = NO_RECT;

  constructor(request: KawibawiboRequest, host: MiniGameHost) {
    super();
    this.#request = request;
    this.#host = host;
    this.putRegInfo = false;
  }

  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_START);
  }
  get leftButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_LEFT);
  }
  get rightButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_RIGHT);
  }
  /** The shown caption of a static (tests). */
  captionOf(id: number): string {
    return this.getDlgItem<Static>(id)?.getTitle() ?? '';
  }
  get myCaption(): string {
    return this.captionOf(WIDC_MY_CHOICE);
  }
  get comCaption(): string {
    return this.captionOf(WIDC_COM_CHOICE);
  }
  get winningCaption(): string {
    return this.captionOf(WIDC_WINNING_COUNT);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#my = this.getWndCtrl(WIDC_STATIC_MY)?.rect ?? NO_RECT;
    this.#com = this.getWndCtrl(WIDC_STATIC_COM)?.rect ?? NO_RECT;
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, this.getClientRect(false));
    this.#sync();
    this.moveParentCenter();
  }

  /** The captions, the icons and the buttons from the machine's view (`RefreshInfo`, `OnDraw`, `EnableAllBtn`). */
  #sync(v: KawibawiboView = this.#request.view()): void {
    this.getDlgItem<Static>(WIDC_WINNING_COUNT)?.setTitle(
      this.#host.fmt(TID_GAME_KWAIBAWIBO_WINCOUNT, v.winning),
    );
    const cap = (hand: number): string =>
      CAPTION_INDENT + this.#host.text(HAND_TIDS[hand] ?? HAND_TIDS[0]);
    this.getDlgItem<Static>(WIDC_MY_CHOICE)?.setTitle(cap(v.myChoice));
    this.getDlgItem<Static>(WIDC_COM_CHOICE)?.setTitle(cap(v.comChoice));
    for (const id of [WIDC_BTN_LEFT, WIDC_BTN_RIGHT, WIDC_BTN_START]) {
      this.getDlgItem<Button>(id)?.enableWindow(v.buttonsEnabled);
    }
    const hand = (side: 1 | 2, choice: number): string =>
      `icon/Icon_${HAND_NAMES[choice] ?? HAND_NAMES[0]}${String(side)}.dds`;
    this.#sprites?.setEntries([
      {
        key: 'my',
        path: hand(1, v.myChoice),
        x: this.#my.l + HAND_AT.x,
        y: this.#my.t + HAND_AT.y,
        w: HAND_SIZE,
        h: HAND_SIZE,
      },
      {
        key: 'com',
        path: hand(2, v.comChoice),
        x: this.#com.l + HAND_AT.x,
        y: this.#com.t + HAND_AT.y,
        w: HAND_SIZE,
        h: HAND_SIZE,
      },
    ]);
  }

  /** `Process()`: the frames since the last call, then the view. */
  override process(now: number): void {
    const frames = this.#clock.advance(now);
    for (let i = 0; i < frames; i += 1) this.#request.frame();
    this.#sync();
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BTN_LEFT) {
        this.#request.cycle(-1);
        this.#sync();
        return true;
      }
      if (id === WIDC_BTN_RIGHT) {
        this.#request.cycle(1);
        this.#sync();
        return true;
      }
      if (id === WIDC_BTN_START) {
        this.#request.start();
        this.#sync();
        return true;
      }
      // the streak, a running round or a draw keep the window
      if (id === WTBID_CLOSE && !this.#request.canClose()) return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed();
    super.onDestroy();
  }
}
