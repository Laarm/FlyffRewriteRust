// `WndRRMiniGameDice` (`CWndRRMiniGameDice`, `src:_Interface/WndRainbowRace.cpp:1241-1539`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-61): `APP_RR_MINIGAME_DICE` (796, 304 x 368), the lucky roller. `STATIC1` shows the target
// number as two digits of `theme/rainbowdicenumber.inc` (22 x 50) at `left + 5` and `left + 29` ("07" keeps the 0); the two dice `Icon/Icon_<n>Dice90X90.dds` stand at `STATIC2` / `STATIC3` + (3, 4). START
// disables itself, rolls (`anim/dice.ts`, 60 Hz frames, counters of the original) and sends the roll packet. Every die-1 face change is the `tick` cue, die 2 ticks only after die 1 finished; FINISH is the `win`
// cue, the end box and the close of the window; a miss enables START again with the backup target (the second packet `{ FAIL, target, -1 }` stores it).

import {
  APP_RR_MINIGAME_DICE,
  WIDC_START,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { DiceRoll, type DiceEvent } from './anim/dice';
import { DICE_NUMBER_ATLAS, iconDice } from './constants';
import { FrameClock } from './frameClock';
import type { DiceRequest } from './ports';
import { mathRng } from './rng';

const ID_PICTURES = 0x5410;
const DIE = 90;

export interface DiceDeps {
  readonly atlases: UiAtlases;
}

export class WndRRMiniGameDice extends WndNeuz {
  readonly appId = APP_RR_MINIGAME_DICE;
  override readonly processEveryFrame = true;
  readonly #request: DiceRequest;
  readonly #deps: DiceDeps;
  readonly #clock = new FrameClock();
  readonly roll: DiceRoll;
  #pictures: SpriteLayer | null = null;

  constructor(request: DiceRequest, deps: DiceDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.roll = new DiceRoll(request.rng ?? mathRng);
    this.roll.target = request.target;
    this.putRegInfo = false;
  }

  get target(): number {
    return this.roll.target;
  }
  get faces(): readonly [number, number] {
    return [this.roll.face1, this.roll.face2];
  }
  get pictureLayer(): SpriteLayer | null {
    return this.#pictures;
  }
  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_START);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<TextCtrl>(WIDC_TEXT1)?.setString(this.#request.text);
    this.#pictures = this.addControl(new SpriteLayer(), ID_PICTURES, this.getClientRect(false));
    this.#paint();
    this.moveParentCenter();
  }

  /** `OnDraw`: the target digits and the two dice. */
  #paint(): void {
    const entries: SpriteEntry[] = [];
    const num = this.getWndCtrl(WIDC_STATIC1)?.rect;
    const dto = this.#deps.atlases.info(DICE_NUMBER_ATLAS);
    if (num && dto) {
      const digits = [Math.trunc(this.roll.target / 10), this.roll.target % 10];
      digits.forEach((d, i) => {
        const cut = dto.sprites[d];
        if (!cut) return;
        entries.push({
          key: `digit${String(i)}`,
          path: dto.texture,
          x: num.l + (i === 0 ? 5 : 29),
          y: num.t,
          w: cut.w,
          h: cut.h,
          src: { x: cut.x, y: cut.y },
        });
      });
    }
    const dice: [number, number][] = [
      [WIDC_STATIC2, this.roll.face1],
      [WIDC_STATIC3, this.roll.face2],
    ];
    dice.forEach(([id, face], i) => {
      const r = this.getWndCtrl(id)?.rect;
      if (!r) return;
      entries.push({
        key: `die${String(i + 1)}`,
        path: iconDice(face),
        x: r.l + 3,
        y: r.t + 4,
        w: DIE,
        h: DIE,
      });
    });
    this.#pictures?.setEntries(entries);
  }

  /** `ReceiveResult( state, p1, p2 )` of a `RainbowRaceMiniGameState` for this game. */
  result(state: number, p1: number, p2: number): void {
    this.roll.receive(state, p1, p2);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_START) {
      this.startButton?.enableWindow(false);
      this.roll.start();
      this.#request.roll();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override process(now: number): void {
    super.process(now);
    const frames = this.#clock.advance(now);
    for (let i = 0; i < frames; i += 1) {
      const events: DiceEvent[] = [];
      this.roll.step(events);
      if (this.#run(events)) return;
    }
  }

  /** Carries out the events of a frame; `true` when the window closed. */
  #run(events: readonly DiceEvent[]): boolean {
    for (const e of events) {
      switch (e.kind) {
        case 'tick':
          this.#request.cue('tick');
          break;
        case 'face':
          this.#paint();
          break;
        case 'win':
          this.#request.cue('win');
          break;
        case 'reset':
          this.startButton?.enableWindow(true);
          this.#paint();
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
