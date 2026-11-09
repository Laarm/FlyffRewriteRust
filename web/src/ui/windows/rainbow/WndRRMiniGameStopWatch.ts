// `WndRRMiniGameStopWatch` (`CWndRRMiniGameStopWatch`, `src:_Interface/WndRainbowRace.cpp:1678-1847`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-62): `APP_RR_MINIGAME_STOPWATCH` (798, 272 x 272). `STATIC3` shows the target
// time "SS : CC" (4570 ms = "04 : 57"), `STATIC2` is the stopwatch picture of the layout with four digits of `theme/rainbowstopwatchnumber.inc` (22 x 50): the elapsed seconds mod 60 (tens, units) at `left + 40`
// and `left + 70`, the hundredths at `left + 135` and `left + 165`, all at `top`. START: the first press records the start time and shows `Buttstop.bmp`, the second shows `ButtStart.bmp` and sends the elapsed ms;
// every frame the running clock advances, and past 60 s it stops, shows `ButtStart.bmp` and sends EXACTLY 60 000 once (the digits then read "00 00": 60 s mod 60). The display keeps the last stopped time.
// The clock is `request.now()` (`ctx.now()`), so the tests run on a fake clock; `request.pinned()` is the e2e pin of the elapsed time.

import {
  APP_RR_MINIGAME_STOPWATCH,
  WIDC_START,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { STOPWATCH_NUMBER_ATLAS } from './constants';
import type { StopwatchRequest } from './ports';
import { digitsOf, StopwatchModel, type StopwatchStep, targetText } from './stopwatch';

const ID_DIGITS = 0x5420;
/** The x offsets of the four digits inside `STATIC2`. */
export const DIGIT_X = [40, 70, 135, 165] as const;
export const BUTTON_START = 'ButtStart.bmp';
export const BUTTON_STOP = 'Buttstop.bmp';

export interface StopwatchDeps {
  readonly atlases: UiAtlases;
}

export class WndRRMiniGameStopWatch extends WndNeuz {
  readonly appId = APP_RR_MINIGAME_STOPWATCH;
  override readonly processEveryFrame = true;
  readonly #request: StopwatchRequest;
  readonly #deps: StopwatchDeps;
  readonly clock = new StopwatchModel();
  #target = 0;
  #layer: SpriteLayer | null = null;

  constructor(request: StopwatchRequest, deps: StopwatchDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get target(): number {
    return this.#target;
  }
  /** The text of `STATIC3` ("SS : CC"). */
  get targetLabel(): string {
    return targetText(this.#target);
  }
  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_START);
  }
  get digitLayer(): SpriteLayer | null {
    return this.#layer;
  }
  /** The digit sprites now drawn (`[sec1, sec2, cs1, cs2]`), read from the display time. */
  get digits(): readonly number[] {
    const d = digitsOf(this.clock.shownMs);
    return [d.sec1, d.sec2, d.cs1, d.cs2];
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<TextCtrl>(WIDC_TEXT1)?.setString(this.#request.text);
    this.#layer = this.addControl(new SpriteLayer(), ID_DIGITS, this.getClientRect(false));
    this.#paint();
    if (this.#request.target !== undefined) this.setTarget(this.#request.target);
    this.moveParentCenter();
  }

  /** `SetTargetTime`. */
  setTarget(ms: number): void {
    this.#target = ms;
    this.getDlgItem<Static>(WIDC_STATIC3)?.setTitle(targetText(ms));
  }

  #paint(): void {
    const back = this.getWndCtrl(WIDC_STATIC2)?.rect;
    const dto = this.#deps.atlases.info(STOPWATCH_NUMBER_ATLAS);
    if (!back || !dto) return;
    const entries: SpriteEntry[] = [];
    this.digits.forEach((d, i) => {
      const cut = dto.sprites[d];
      if (!cut) return;
      entries.push({
        key: `digit${String(i)}`,
        path: dto.texture,
        x: back.l + (DIGIT_X[i] ?? 0),
        y: back.t,
        w: cut.w,
        h: cut.h,
        src: { x: cut.x, y: cut.y },
      });
    });
    this.#layer?.setEntries(entries);
  }

  #apply(step: StopwatchStep): void {
    this.startButton?.setTexture(step.running ? BUTTON_STOP : BUTTON_START);
    if (step.send !== undefined) this.#request.send(step.send);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_START) {
      this.#apply(this.clock.press(this.#request.now(), this.#request.pinned?.() ?? null));
      this.#paint();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `Process`: the running clock advances; past 60 s it stops and sends 60 000. */
  override process(now: number): void {
    super.process(now);
    if (!this.clock.running) return;
    const step = this.clock.process(this.#request.now(), this.#request.pinned?.() ?? null);
    this.#paint();
    if (!step.running) this.#apply(step);
  }
}
