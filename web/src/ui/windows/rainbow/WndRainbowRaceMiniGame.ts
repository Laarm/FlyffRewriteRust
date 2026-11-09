// `WndRainbowRaceMiniGame` (`CWndRainbowRaceMiniGame`, `src:_Interface/WndRainbowRace.cpp:321-605`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-60): `APP_RAINBOWRACE_MINIGAME` (792, 320 x 368), the progress list of the
// seven games. Seven title statics `STATIC1-7` carry the game names at `( left + 10, top + 5 )` in `0xff858585` (completed) or `0xff7878c8`; a completed game draws its `Theme/Check<Colour>.bmp` on the check
// static `STATIC8-14`; the current game gets the pulsing 8-px frame in its colour (lerp 0..1 by 0.04 per frame and back). A click on a title opens the information window 794 with that game's panel
// (`request.info( game )`); the select cursor shows over the titles; OK closes, and the information window goes with this one.
//
// The window shows the live slice (`request.state()`) on every `process`, except while it is FROZEN (a FINISH arrived while it was open: it keeps its snapshot until `unfreeze()`, the legacy
// `m_bGameInfoRefresh = FALSE`); a newly created window is never frozen. Placement under the race button (`progressPosition`).

import {
  APP_RAINBOWRACE_BUTTON,
  APP_RAINBOWRACE_MINIGAME,
  APP_RAINBOWRACE_MINIGAME_INFO,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC7,
  WIDC_STATIC8,
  WIDC_STATIC9,
  WIDC_STATIC10,
  WIDC_STATIC11,
  WIDC_STATIC12,
  WIDC_STATIC13,
  WIDC_STATIC14,
} from '../../../gen/defs/resdata';
import { RectLayer } from '../../controls/RectLayer';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectHeight, rectWidth } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  CHECK_PICTURES,
  GAME_COLORS,
  GAME_TITLE_TIDS,
  RMG_MAX,
  TITLE_COLOR_DONE,
  TITLE_COLOR_OPEN,
} from './constants';
import { FrameClock } from './frameClock';
import { gameKeyOf, glowStrips } from './glow';
import { progressPosition } from './geometry';
import { Pulse } from './ladder';
import type { ProgressRequest, RainbowHost } from './ports';

const TITLE_IDS = [
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC7,
] as const;
const CHECK_IDS = [
  WIDC_STATIC8,
  WIDC_STATIC9,
  WIDC_STATIC10,
  WIDC_STATIC11,
  WIDC_STATIC12,
  WIDC_STATIC13,
  WIDC_STATIC14,
] as const;
const ID_TEXT = 0x5310;
const ID_CHECKS = 0x5311;
const ID_GLOW = 0x5312;
/** `CUR_SELECT`. */
const CUR_SELECT = 10;

export interface ProgressDeps {
  readonly host: RainbowHost;
}

export class WndRainbowRaceMiniGame extends WndNeuz {
  readonly appId = APP_RAINBOWRACE_MINIGAME;
  override readonly processEveryFrame = true;
  readonly #request: ProgressRequest;
  readonly #host: RainbowHost;
  readonly #clock = new FrameClock();
  readonly #pulse = new Pulse(0.04);
  #frozen = false;
  #current = -1;
  #done: boolean[] = new Array<boolean>(RMG_MAX).fill(false);
  #titles: Rect[] = [];
  #text: TextLayer | null = null;
  #checks: SpriteLayer | null = null;
  #glow: RectLayer | null = null;
  #pointer: Point | null = null;

  constructor(request: ProgressRequest, deps: ProgressDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get frozen(): boolean {
    return this.#frozen;
  }
  /** `m_nCurrentGame`. */
  get current(): number {
    return this.#current;
  }
  /** `m_bIsCompletedGame[ game ]`. */
  completed(game: number): boolean {
    return this.#done[game] === true;
  }
  get textLayer(): TextLayer | null {
    return this.#text;
  }
  get checkLayer(): SpriteLayer | null {
    return this.#checks;
  }
  get glowLayer(): RectLayer | null {
    return this.#glow;
  }
  /** The layout rect of the title of a game. */
  titleRect(game: number): Rect | null {
    return this.#titles[game] ?? null;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#titles = TITLE_IDS.map((id) => this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 });
    const client = this.getClientRect(false);
    this.#text = this.addControl(new TextLayer(), ID_TEXT, client);
    this.#checks = this.addControl(new SpriteLayer(), ID_CHECKS, client);
    this.#glow = this.addControl(new RectLayer(), ID_GLOW, client);
    this.#refresh();
    this.#paint();
    // under (or above) the race button when there is one
    const button = this.env.wm.get(APP_RAINBOWRACE_BUTTON);
    if (button) {
      this.move(
        progressPosition(this.env.wm.layoutRect, button.rectWindow, {
          x: rectWidth(this.rectWindow),
          y: rectHeight(this.rectWindow),
        }),
      );
    }
  }

  // ---- the state ------------------------------------------------------------------------------------------------------------------

  /** `SetCurrentGameInfo`: the snapshot is the live slice now. */
  #refresh(): void {
    const s = this.#request.state();
    this.#current = gameKeyOf(s.nowGame);
    this.#done = Array.from({ length: RMG_MAX }, (_, i) => (s.completed & (1 << i)) !== 0);
  }

  /** `m_bGameInfoRefresh = FALSE` (a FINISH arrived): the window keeps what it shows. */
  freeze(): void {
    this.#frozen = true;
  }

  /** `m_bGameInfoRefresh = TRUE`: the next frame reads the slice again. */
  unfreeze(): void {
    this.#frozen = false;
  }

  /** `SetCurrentGameInfo()` at once (the OK of an end box, while the window is still frozen). */
  refreshNow(): void {
    this.#refresh();
    this.#paint();
  }

  // ---- the pixels ------------------------------------------------------------------------------------------------------------------

  #paint(): void {
    const items: TextItem[] = [];
    const checks: SpriteEntry[] = [];
    for (let i = 0; i < RMG_MAX; i += 1) {
      const r = this.#titles[i];
      if (!r) continue;
      const done = this.#done[i] === true;
      items.push({
        x: r.l + 10,
        y: r.t + 5,
        text: this.#host.text(GAME_TITLE_TIDS[i] ?? 0),
        color: done ? TITLE_COLOR_DONE : TITLE_COLOR_OPEN,
      });
      const c = this.getWndCtrl(CHECK_IDS[i] ?? 0)?.rect;
      if (done && i !== this.#current && c) {
        checks.push({
          key: `check${String(i)}`,
          path: CHECK_PICTURES[i] ?? null,
          x: c.l,
          y: c.t,
          w: 32,
          h: 32,
        });
      }
    }
    this.#text?.setItems(items);
    this.#checks?.setEntries(checks);
    const cur = this.#titles[this.#current];
    const rgb = GAME_COLORS[this.#current];
    this.#glow?.setEntries(
      cur && rgb ? glowStrips(`glow${String(this.#current)}`, cur, rgb, this.#pulse.lerp) : [],
    );
  }

  /** Every frame: the pulse advances and, unless frozen, the slice is read again. */
  override process(now: number): void {
    super.process(now);
    const frames = this.#clock.advance(now);
    for (let i = 0; i < frames; i += 1) this.#pulse.tick();
    if (!this.#frozen) this.#refresh();
    this.#paint();
  }

  // ---- the pointer -------------------------------------------------------------------------------------------------------------------

  #titleAt(pt: Point): number {
    return this.#titles.findIndex((r) => pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b);
  }

  override onMouseMove(pt: Point): void {
    this.#pointer = pt;
  }
  override onMouseLeave(): void {
    this.#pointer = null;
  }

  /** `OnSetCursor`: the select cursor over a title. */
  uiCursor(): number | null {
    return this.#pointer !== null && this.#titleAt(this.#pointer) >= 0 ? CUR_SELECT : null;
  }

  override onLButtonUp(pt: Point): void {
    const game = this.#titleAt(pt);
    if (game >= 0) this.#request.info(game);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** The information window belongs to this one (`~CWndRainbowRaceMiniGame`: `SAFE_DELETE( m_pWndMiniGameInfo )`). */
  override onDestroy(): void {
    this.env.wm.get(APP_RAINBOWRACE_MINIGAME_INFO)?.destroy();
    super.onDestroy();
  }
}
