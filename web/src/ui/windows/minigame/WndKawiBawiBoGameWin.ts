// `WndKawiBawiBoGameWin` (`CWndKawiBawiBoGameWin`, `src:_Interface/WndMiniGame.cpp:320-528`, ref/22 §17 P; WP-UI-63): `APP_MINIGAME_KAWIBAWIBO_WIN` (620, 256 x 336, "You Win!"), the window of a won
// round. It stands 10 px right of the game window, top-aligned with it, or `10 + width` to its left when the layout has no room on the right. `STATIC3` / `STATIC4` read
// `TID_GMAE_KAWIBAWIBO_CUR_PRESENT` (sic) / `TID_GAME_KWAIBAWIBO_NXT_PRESENT`, `TEXT_GUID` holds `TID_GAME_KAWIBAWIBO_GUID1` and, in red, `..._GUID2`. The prize of the streak and the next
// one are drawn at `CURRENT_PRESENT` / `NEXT_PRESENT` + (16, 16) (32 x 32 item icons); a count above 1 is printed at the icon's bottom right (`TID_GAME_KWAIBAWIBO_PRESENT_NUM`, blue
// `0xff0000ff`, at `x + 36 - width`, `y + 48 - height`) and an item tooltip follows the hover. `BTN_END` takes the prize, `BTN_NEXT` (disabled at the highest streak) plays on; the title
// CLOSE button is refused. Whichever button closes the window, closing it re-enables the buttons of the game window (the request's `closed`).

import {
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_KAWIBAWIBO_WIN,
  WIDC_BTN_END,
  WIDC_BTN_NEXT,
  WIDC_CURRENT_PRESENT,
  WIDC_NEXT_PRESENT,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_TEXT_GUID,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_KAWIBAWIBO_GUID1,
  TID_GAME_KAWIBAWIBO_GUID2,
  TID_GAME_KWAIBAWIBO_NXT_PRESENT,
  TID_GAME_KWAIBAWIBO_PRESENT_NUM,
  TID_GMAE_KAWIBAWIBO_CUR_PRESENT,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset, rectWidth } from '../../layout/geometry';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import { FONT, lineHeight, textWidth } from '../../text/fonts';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { KawibawiboWinRequest, MiniGameHost, PrizeView } from './ports';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_SPRITES = 0x7b20;
const ID_TEXTS = 0x7b21;
/** The gap to the game window and the icon placement inside the statics. */
const GAP = 10;
const ICON_AT = 16;
const ICON_SIZE = 32;
const COUNT_COLOR = 0xff0000ff;
const GUID2_COLOR = 0xffff0000;

export interface WinDeps {
  readonly host: MiniGameHost;
  readonly tooltip: TooltipService;
}

export class WndKawiBawiBoGameWin extends WndNeuz {
  readonly appId = APP_MINIGAME_KAWIBAWIBO_WIN;
  readonly #request: KawibawiboWinRequest;
  readonly #deps: WinDeps;
  #cur: Rect = NO_RECT;
  #next: Rect = NO_RECT;
  #sprites: SpriteLayer | null = null;
  #texts: TextLayer | null = null;

  constructor(request: KawibawiboWinRequest, deps: WinDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get endButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_END);
  }
  get nextButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_NEXT);
  }
  get guide(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT_GUID);
  }
  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }
  captionOf(id: number): string {
    return this.getDlgItem<Static>(id)?.getTitle() ?? '';
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // 10 px right of the game window (top-aligned), or left of it when the layout has no room
    const game = this.env.wm.get(APP_MINIGAME_KAWIBAWIBO);
    if (game) {
      const root = this.env.wm.layoutRect;
      const rg = game.getScreenRect();
      const width = rectWidth(this.rectWindow);
      const at: Point =
        root.r - rg.r < width ? { x: rg.l - (GAP + width), y: rg.t } : { x: rg.r + GAP, y: rg.t };
      this.move(at);
    }
    const host = this.#deps.host;
    this.getDlgItem<Static>(WIDC_STATIC3)?.setTitle(host.text(TID_GMAE_KAWIBAWIBO_CUR_PRESENT));
    this.getDlgItem<Static>(WIDC_STATIC4)?.setTitle(host.text(TID_GAME_KWAIBAWIBO_NXT_PRESENT));
    const guide = this.guide;
    guide?.setString(host.text(TID_GAME_KAWIBAWIBO_GUID1));
    guide?.addString(host.text(TID_GAME_KAWIBAWIBO_GUID2), GUID2_COLOR);
    this.#cur = this.getWndCtrl(WIDC_CURRENT_PRESENT)?.rect ?? NO_RECT;
    this.#next = this.getWndCtrl(WIDC_NEXT_PRESENT)?.rect ?? NO_RECT;
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    if (this.#request.nextDisabled) this.nextButton?.enableWindow(false);
    this.#paint();
  }

  #paint(): void {
    const sprites: SpriteEntry[] = [];
    const texts: TextItem[] = [];
    const draw = (key: string, rect: Rect, prize: PrizeView): void => {
      // an item without a prop draws nothing (`if( pItemProp != NULL )`)
      if (prize.icon === null) return;
      const x = rect.l + ICON_AT;
      const y = rect.t + ICON_AT;
      sprites.push({ key, path: prize.icon, x, y, w: ICON_SIZE, h: ICON_SIZE });
      if (prize.count > 1) {
        const text = this.#deps.host.fmt(TID_GAME_KWAIBAWIBO_PRESENT_NUM, prize.count);
        const w = textWidth(FONT.text, text);
        texts.push({ x: x + 36 - w, y: y + 48 - lineHeight(FONT.text), text, color: COUNT_COLOR });
      }
    };
    draw('cur', this.#cur, this.#request.prize);
    draw('next', this.#next, this.#request.nextPrize);
    this.#sprites?.setEntries(sprites);
    this.#texts?.setItems(texts);
  }

  /** `OnMouseWndSurface`: the item tooltips over the two prize frames. */
  override onMouseMove(pt: Point): void {
    const pick = (rect: Rect, prize: PrizeView): boolean => {
      if (!rectContains(rect, pt) || prize.tooltip === null) return false;
      const s = this.getScreenClientRect();
      this.#deps.tooltip.putItem(
        this,
        prize.tooltip,
        rectOffset(rect, s.l, s.t),
        APP_MINIGAME_KAWIBAWIBO_WIN,
      );
      return true;
    };
    if (!pick(this.#cur, this.#request.prize)) pick(this.#next, this.#request.nextPrize);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BTN_END) {
        this.#request.end();
        this.destroy();
        return true;
      }
      if (id === WIDC_BTN_NEXT) {
        this.#request.next();
        this.destroy();
        return true;
      }
      // the close button of the title bar is refused
      if (id === WTBID_CLOSE) return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed();
    super.onDestroy();
  }
}
