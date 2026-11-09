// `WndRRMiniGameKawiBawiBo` (`CWndRRMiniGameKawiBawiBo`, `src:_Interface/WndRainbowRace.cpp:941-1235`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-61): `APP_RR_MINIGAME_KAWIBAWIBO` (795, 304 x 384), rock-paper-scissors against
// the server. `WINNING_COUNT` reads "`n` win(s)", `MY_CHOICE1` / `COM_CHOICE1` the hand names (8 spaces + `TID_GAME_KAWIBAWIBO_*`), the pictures `Icon/Icon_<Kawi|Bawi|Bo>1.dds` / `...2.dds` stand at the picture
// statics + (3, 8). `LBUTTON` / `RBUTTON` cycle the own hand, START disables the three buttons, starts the spin and sends the play packet (the hand is NOT sent: the server rolls and the client derives the
// computer's hand from the answer). The spin is `anim/kawibawibo.ts` over 60 Hz frames; every spin step is the `tick` cue, TRUE / FINISH the `win` cue; FINISH opens the end box and closes the window, FAIL the
// DEFEAT box and the streak is 0.

import {
  APP_RR_MINIGAME_KAWIBAWIBO,
  WIDC_BTN_START,
  WIDC_COM_CHOICE1,
  WIDC_LBUTTON,
  WIDC_MY_CHOICE1,
  WIDC_RBUTTON,
  WIDC_STATIC_COM1,
  WIDC_STATIC_MY1,
  WIDC_TEXT1,
  WIDC_WINNING_COUNT,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_KAWIBAWIBO_BAWI,
  TID_GAME_KAWIBAWIBO_BO,
  TID_GAME_KAWIBAWIBO_DEFEAT,
  TID_GAME_KAWIBAWIBO_KAWI,
  TID_GAME_KWAIBAWIBO_WINCOUNT,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { type SpinEvent, KawiBawiBoSpin } from './anim/kawibawibo';
import { iconKawiBawiBo } from './constants';
import { FrameClock } from './frameClock';
import type { KawiBawiBoRequest, RainbowHost } from './ports';

const ID_PICTURES = 0x5400;
const HAND_TIDS = [TID_GAME_KAWIBAWIBO_KAWI, TID_GAME_KAWIBAWIBO_BAWI, TID_GAME_KAWIBAWIBO_BO];
/** The size of the hand pictures. */
const PICTURE = 90;

export interface KawiBawiBoDeps {
  readonly host: RainbowHost;
}

export class WndRRMiniGameKawiBawiBo extends WndNeuz {
  readonly appId = APP_RR_MINIGAME_KAWIBAWIBO;
  override readonly processEveryFrame = true;
  readonly #request: KawiBawiBoRequest;
  readonly #host: RainbowHost;
  readonly #clock = new FrameClock();
  readonly spin = new KawiBawiBoSpin();
  #pictures: SpriteLayer | null = null;

  constructor(request: KawiBawiBoRequest, deps: KawiBawiBoDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get wins(): number {
    return this.spin.wins;
  }
  get myChoice(): number {
    return this.spin.my;
  }
  get comChoice(): number {
    return this.spin.com;
  }
  get pictureLayer(): SpriteLayer | null {
    return this.#pictures;
  }
  button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }
  get leftButton(): Button | null {
    return this.button(WIDC_LBUTTON);
  }
  get rightButton(): Button | null {
    return this.button(WIDC_RBUTTON);
  }
  get startButton(): Button | null {
    return this.button(WIDC_BTN_START);
  }
  stat(id: number): Static | null {
    return this.getDlgItem<Static>(id);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<TextCtrl>(WIDC_TEXT1)?.setString(this.#request.text);
    this.#pictures = this.addControl(new SpriteLayer(), ID_PICTURES, this.getClientRect(false));
    this.#refresh();
    this.moveParentCenter();
  }

  /** `RefreshInfo`: the streak, the two hand names and the two pictures. */
  #refresh(): void {
    const hand = (i: number): string => `        ${this.#host.text(HAND_TIDS[i] ?? 0)}`;
    this.stat(WIDC_WINNING_COUNT)?.setTitle(
      this.#host.fmt(TID_GAME_KWAIBAWIBO_WINCOUNT, this.spin.wins),
    );
    this.stat(WIDC_MY_CHOICE1)?.setTitle(hand(this.spin.my));
    this.stat(WIDC_COM_CHOICE1)?.setTitle(hand(this.spin.com));
    const my = this.getWndCtrl(WIDC_STATIC_MY1)?.rect;
    const com = this.getWndCtrl(WIDC_STATIC_COM1)?.rect;
    this.#pictures?.setEntries([
      ...(my
        ? [
            {
              key: 'my',
              path: iconKawiBawiBo(this.spin.my, 1),
              x: my.l + 3,
              y: my.t + 8,
              w: PICTURE,
              h: PICTURE,
            },
          ]
        : []),
      ...(com
        ? [
            {
              key: 'com',
              path: iconKawiBawiBo(this.spin.com, 2),
              x: com.l + 3,
              y: com.t + 8,
              w: PICTURE,
              h: PICTURE,
            },
          ]
        : []),
    ]);
  }

  #enableButtons(on: boolean): void {
    this.leftButton?.enableWindow(on);
    this.rightButton?.enableWindow(on);
    this.startButton?.enableWindow(on);
  }

  /** `ReceiveResult( state )` of a `RainbowRaceMiniGameState` for this game. */
  result(state: number): void {
    this.spin.receive(state);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_LBUTTON) {
        this.spin.my = this.spin.my - 1 < 0 ? 2 : this.spin.my - 1;
        this.#refresh();
        return true;
      }
      if (id === WIDC_RBUTTON) {
        this.spin.my = this.spin.my + 1 > 2 ? 0 : this.spin.my + 1;
        this.#refresh();
        return true;
      }
      if (id === WIDC_BTN_START) {
        this.#enableButtons(false);
        this.spin.start();
        this.#request.play();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** The frames of the spin (`Process`). */
  override process(now: number): void {
    super.process(now);
    const frames = this.#clock.advance(now);
    for (let i = 0; i < frames; i += 1) {
      const events: SpinEvent[] = [];
      this.spin.step(events);
      if (this.#run(events)) return;
    }
  }

  /** Carries out the events of a frame; `true` when the window closed. */
  #run(events: readonly SpinEvent[]): boolean {
    for (const e of events) {
      switch (e.kind) {
        case 'tick':
          this.#request.cue('tick');
          break;
        case 'com':
        case 'wins':
          this.#refresh();
          break;
        case 'win':
          this.#request.cue('win');
          break;
        case 'defeat':
          this.#host.box(this.#host.text(TID_GAME_KAWIBAWIBO_DEFEAT));
          break;
        case 'enable':
          this.#enableButtons(true);
          break;
        case 'finish':
          this.#request.end();
          this.destroy();
          return true;
      }
    }
    return false;
  }
}
