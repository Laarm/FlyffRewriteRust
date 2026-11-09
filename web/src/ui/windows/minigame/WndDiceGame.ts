// `WndDiceGame` (`CWndDiceGame`, `src:_Interface/WndMiniGame.cpp:908-1390`, ref/22 §17 P; WP-UI-63): `APP_MINIGAME_DICE` (616, 320 x 512, "Roll the Dice!"), the "Five System" dice game. The big die
// (`Icon_<n>Dice90X90.dds`, 90 x 90) stands at `STATIC_DICE` + (3, 4); the six small faces (`Icon_<n>Dice<n>.dds`, 20 x 20) at `STATIC14..19`; per face a stake text (`COST_NUM<i>`, the stake with
// thousands separators in `0xff4d4dff`) and a payout text (`RCOST_NUM<i>`, stake x multiple in `0xff9932cd`), both disabled text controls, and a stake button (`BTN_NUM<i>`) that opens the stake
// box. Start (`BTN_START`) is disabled until the server named the limits (`FIVESYSTEM_OPENWND`); `TEXT_DESC` shows the panel `minigamedicedesc`. After the roll the three rectangles of the face
// that came up (`STATIC2+i`, `STATIC8+i`, `STATIC14+i` less 3 px on the bottom and right) flash `(240, 255, 255)` between alpha 70 and 230. The spin, the sends and the result are the machine's
// (`features/events/minigame/dice.ts`); the window counts 60 Hz frames for it. Opening sends `FiveSystemOpenWnd` (the request's `opened`), closing `FiveSystemDestroyWnd` (the machine's `closed`).
// Esc keeps the window (`ESC_KEEP_APPS`).

import {
  APP_MINIGAME_DICE,
  WIDC_BTN_START,
  WIDC_STATIC_DICE,
  WIDC_TEXT_DESC,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import type { Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { FrameClock } from './frames';
import { DICE_IDS, PAYOUT_COLOR, STAKE_COLOR } from './rows';
import type { DicePort, MiniGameHost } from './ports';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_SPRITES = 0x7b50;
const ID_FLASH = 0x7b51;
export class WndDiceGame extends WndNeuz {
  readonly appId = APP_MINIGAME_DICE;
  override readonly processEveryFrame = true;
  readonly #port: DicePort;
  readonly #host: MiniGameHost;
  readonly #commas: (n: number) => string;
  readonly #clock = new FrameClock();
  #sprites: SpriteLayer | null = null;
  #flash: RectLayer | null = null;
  #dice: Rect = NO_RECT;
  #description = '';

  constructor(port: DicePort, host: MiniGameHost, commas: (n: number) => string) {
    super();
    this.#port = port;
    this.#host = host;
    this.#commas = commas;
    this.putRegInfo = false;
  }

  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_START);
  }
  get descriptionText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT_DESC);
  }
  stakeButton(face: number): Button | null {
    const id = DICE_IDS.buttons[face];
    return id === undefined ? null : this.getDlgItem<Button>(id);
  }
  costText(face: number): TextCtrl | null {
    const id = DICE_IDS.cost[face];
    return id === undefined ? null : this.getDlgItem<TextCtrl>(id);
  }
  payoutText(face: number): TextCtrl | null {
    const id = DICE_IDS.rcost[face];
    return id === undefined ? null : this.getDlgItem<TextCtrl>(id);
  }
  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  get flashLayer(): RectLayer | null {
    return this.#flash;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#dice = this.getWndCtrl(WIDC_STATIC_DICE)?.rect ?? NO_RECT;
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    this.#flash = this.addControl(new RectLayer(), ID_FLASH, client);
    // the cost texts are display only
    for (let i = 0; i < 6; i += 1) {
      this.costText(i)?.enableWindow(false);
      this.payoutText(i)?.enableWindow(false);
    }
    this.startButton?.enableWindow(false);
    this.#port.opened?.();
    this.#sync();
    this.moveParentCenter();
  }

  /** The rect of a control by id in client coordinates. */
  #rect(id: number): Rect {
    return this.getWndCtrl(id)?.rect ?? NO_RECT;
  }

  #sync(): void {
    const v = this.#port.view();
    this.startButton?.enableWindow(v.startEnabled);
    for (let i = 0; i < 6; i += 1) {
      this.stakeButton(i)?.enableWindow(v.buttonsEnabled);
      const stake = v.stakes[i] ?? 0;
      this.costText(i)?.setString(this.#commas(stake), STAKE_COLOR);
      this.payoutText(i)?.setString(this.#commas(stake * v.multiple), PAYOUT_COLOR);
    }
    if (v.description !== this.#description) {
      this.#description = v.description;
      this.descriptionText?.setParsingString(v.description);
    }
    const entries: SpriteEntry[] = [
      {
        key: 'big',
        path: `icon/Icon_${String(v.face + 1)}Dice90X90.dds`,
        x: this.#dice.l + 3,
        y: this.#dice.t + 4,
        w: 90,
        h: 90,
      },
    ];
    for (let i = 0; i < 6; i += 1) {
      const r = this.#rect(DICE_IDS.small[i] ?? 0);
      entries.push({
        key: `small${String(i)}`,
        path: `icon/Icon_${String(i + 1)}Dice${String(i + 1)}.dds`,
        x: r.l,
        y: r.t,
        w: 20,
        h: 20,
      });
    }
    this.#sprites?.setEntries(entries);
    this.#paintFlash(v.flashFace, v.flashAlpha);
  }

  #paintFlash(face: number | null, alpha: number): void {
    if (face === null) {
      this.#flash?.setEntries([]);
      return;
    }
    const fill = `rgba(240, 255, 255, ${(alpha / 255).toFixed(3)})`;
    const stake = this.#rect(DICE_IDS.stake[face] ?? 0);
    const payout = this.#rect(DICE_IDS.payout[face] ?? 0);
    const small = this.#rect(DICE_IDS.small[face] ?? 0);
    // "the real image is smaller than the rect": 3 px less on the bottom and right
    const rects: readonly [string, Rect][] = [
      ['stake', stake],
      ['payout', payout],
      ['face', { l: small.l, t: small.t, r: small.r - 3, b: small.b - 3 }],
    ];
    this.#flash?.setEntries(
      rects.map(([key, r]): RectEntry => ({
        key,
        x: r.l,
        y: r.t,
        w: r.r - r.l,
        h: r.b - r.t,
        fill,
      })),
    );
  }

  /** `Process()`: the frames since the last call (and the flash with them), then the view. */
  override process(now: number): void {
    const frames = this.#clock.advance(now);
    for (let i = 0; i < frames; i += 1) this.#port.frame();
    this.#sync();
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const face = (DICE_IDS.buttons as readonly number[]).indexOf(id);
      if (face >= 0) {
        this.#port.openStake(face);
        this.#sync();
        return true;
      }
      if (id === WIDC_BTN_START) {
        this.#port.start();
        this.#sync();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#port.closed();
    super.onDestroy();
  }

  /** The texts and boxes the window was built with (tests). */
  get host(): MiniGameHost {
    return this.#host;
  }
}
