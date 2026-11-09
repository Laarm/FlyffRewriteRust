// `WndRRMiniGameLadder` (`CWndRRMiniGameLadder`, `src:_Interface/WndRainbowRace.cpp:2192-2750`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-62): `APP_RR_MINIGAME_LADDER` (801, 528 x 560), "Drop the Ball!". 15 start cells
// (`CUSTOM1-15`, 24 x 24 at `8 + 34 i`) over the field `CUSTOM_BG` (8, 34)-(508, 334) and 15 goal cells 327 px below them. Before the start a click on a start cell chooses it (the ball picture stands on it,
// default 0). OK disables itself, starts and sends the roll packet. The result (`result( state )`) draws the table of bars (`makeLadder`), turns the background to `...Bg2.bmp`, puts the goal box (FINISH) or a
// random other box (FAIL) at the bottom and lets the ball run (`LadderRun`, 3 px down and 2 px sideways per 60 Hz frame) leaving the pulsing red trail (lerp 0.1 per frame). At the end FINISH opens the end box
// and closes the window, FAIL opens the fail box whose destruction resets the window (start 0, OK enabled).

import {
  APP_RR_MINIGAME_LADDER,
  WIDC_CUSTOM_BG,
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
  WIDC_OK,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { LADDER_PICTURES, MP } from './constants';
import { FrameClock } from './frameClock';
import {
  emptyLadder,
  LADDER_COLS,
  LADDER_ROWS,
  type LadderGeometry,
  LadderRun,
  makeLadder,
  Pulse,
} from './ladder';
import type { LadderRequest } from './ports';
import { glowStrips } from './glow';
import { mathRng } from './rng';

const STARTS = [
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
] as const;
const ID_BOARD = 0x5440;
const ID_TRAIL = 0x5441;
const ID_BALL = 0x5442;
const CUR_SELECT = 10;
/** The goal row lies this far below the start row. */
export const GOAL_OFFSET = 327;
const BOX = 24;

export interface LadderDeps {
  /** Opens the fail box (`CWndRRMiniGameLadderFail`); `onGone` runs when it is destroyed. */
  openFail(onGone: () => void): void;
}

export class WndRRMiniGameLadder extends WndNeuz {
  readonly appId = APP_RR_MINIGAME_LADDER;
  override readonly processEveryFrame = true;
  readonly #request: LadderRequest;
  readonly #deps: LadderDeps;
  readonly #clock = new FrameClock();
  readonly #pulse = new Pulse(0.1);
  #starts: Rect[] = [];
  #bg: Rect = { l: 0, t: 0, r: 0, b: 0 };
  #board: SpriteLayer | null = null;
  #trail: RectLayer | null = null;
  #ball: SpriteLayer | null = null;
  #pointer: Point | null = null;
  /** `m_nStart`. */
  start = 0;
  /** `m_bStart`. */
  started = false;
  /** `m_nResult` (`MP_*`), -1 while none. */
  resultState = -1;
  /** `m_nTarget`. */
  target = 0;
  /** `m_nLadder` (`makeLadder` fills it). */
  ladder: number[][] = emptyLadder();
  run: LadderRun | null = null;
  #failShown = false;
  #ended = false;

  constructor(request: LadderRequest, deps: LadderDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get boardLayer(): SpriteLayer | null {
    return this.#board;
  }
  get trailLayer(): RectLayer | null {
    return this.#trail;
  }
  get ballLayer(): SpriteLayer | null {
    return this.#ball;
  }
  get startRects(): readonly Rect[] {
    return this.#starts;
  }
  get geometry(): LadderGeometry {
    return {
      startLeft: this.#starts.map((r) => r.l),
      startTop: this.#starts[0]?.t ?? 0,
      bgTop: this.#bg.t,
      bgBottom: this.#bg.b,
    };
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<TextCtrl>(WIDC_TEXT1)?.setString(this.#request.text);
    this.#starts = STARTS.map((id) => this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 });
    this.#bg = this.getWndCtrl(WIDC_CUSTOM_BG)?.rect ?? { l: 0, t: 0, r: 0, b: 0 };
    const client = this.getClientRect(false);
    this.#board = this.addControl(new SpriteLayer(), ID_BOARD, client);
    this.#trail = this.addControl(new RectLayer(), ID_TRAIL, client);
    this.#ball = this.addControl(new SpriteLayer(), ID_BALL, client);
    this.reset();
    this.moveParentCenter();
  }

  /** `Reset`: back to the start (the table of bars is empty, OK works again). */
  reset(): void {
    if (this.destroyed) return;
    this.resultState = -1;
    this.start = 0;
    this.target = 0;
    this.started = false;
    this.run = null;
    this.#failShown = false;
    this.#ended = false;
    this.ladder = emptyLadder();
    this.okButton?.enableWindow(true);
    this.#paint();
  }

  /** `ReceiveResult( state )`: draws the table and starts the run. */
  result(state: number): void {
    if (!this.started) return;
    this.resultState = state;
    const made = makeLadder(this.#request.rng ?? mathRng, this.start, state === MP.FINISH);
    this.ladder = made.ladder;
    this.target = made.target;
    this.run = new LadderRun(this.geometry, made.ladder, this.start);
    this.#paint();
  }

  // ---- the pixels --------------------------------------------------------------------------------------------------------------------

  #paint(): void {
    const sprites: SpriteEntry[] = [];
    const pic = (key: string, path: string, x: number, y: number, w: number, h: number): void => {
      sprites.push({ key, path, x, y, w, h });
    };
    for (let i = 0; i < LADDER_COLS * 2; i += 1) {
      const r = this.#starts[i % LADDER_COLS];
      if (!r) continue;
      pic(
        `choice${String(i)}`,
        LADDER_PICTURES.choice,
        r.l,
        r.t + (i < LADDER_COLS ? 0 : GOAL_OFFSET),
        BOX,
        BOX,
      );
    }
    const running = this.started && this.run !== null;
    pic(
      'bg',
      running ? LADDER_PICTURES.bg2 : LADDER_PICTURES.bg1,
      this.#bg.l,
      this.#bg.t,
      500,
      300,
    );
    if (running) {
      const nStartGap = this.#bg.t + 32;
      for (let i = 0; i < LADDER_COLS; i += 1) {
        for (let j = 0; j < LADDER_ROWS; j += 1) {
          if ((this.ladder[i]?.[j] ?? -1) > i) {
            pic(
              `rung${String(i)}-${String(j)}`,
              LADDER_PICTURES.horizon,
              18 + 34 * i,
              nStartGap + 25 * j - 2,
              37,
              3,
            );
          }
        }
      }
      for (let i = 0; i < LADDER_COLS; i += 1) {
        const r = this.#starts[i];
        if (!r) continue;
        pic(
          `end${String(i)}`,
          i === this.target ? LADDER_PICTURES.goal : LADDER_PICTURES.fail,
          r.l,
          r.t + GOAL_OFFSET,
          BOX,
          BOX,
        );
      }
    }
    this.#board?.setEntries(sprites);
    this.#paintMoving();
  }

  /** The trail and the ball (they change on every frame). */
  #paintMoving(): void {
    const trail: RectEntry[] = [];
    const run = this.run;
    const start = this.#starts[this.start];
    const lerp = this.#pulse.lerp;
    // `FillRect( .., 0x00ff0000, 0xbbff0000 )`: the colour runs from alpha 0xbb (lerp 0) to 0 (lerp 1)
    const fade = (1 - lerp) * (0xbb / 255);
    if (run) {
      run.lines.forEach((r, i) => {
        trail.push(...glowStrips(`line${String(i)}`, r, [255, 0, 0], fade));
      });
      trail.push(...glowStrips('head', run.line, [255, 0, 0], fade));
    } else if (start) {
      // before the run: the stub under the chosen start cell (`m_rectPointLine`)
      trail.push(
        ...glowStrips(
          'head',
          { l: start.l + 9, r: start.l + 14, t: start.t + 24, b: start.t + 24 },
          [255, 0, 0],
          fade,
        ),
      );
    }
    this.#trail?.setEntries(trail);
    const at = run ? run.pt : start ? { x: start.l, y: start.t } : null;
    this.#ball?.setEntries(
      at ? [{ key: 'ball', path: LADDER_PICTURES.point, x: at.x, y: at.y, w: BOX, h: BOX }] : [],
    );
  }

  // ---- the pointer ---------------------------------------------------------------------------------------------------------------------

  /** The start cell under a client point, -1 for none. */
  startAt(pt: Point): number {
    return this.#starts.findIndex((r) => pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b);
  }

  override onLButtonUp(pt: Point): void {
    if (this.started) return;
    const i = this.startAt(pt);
    if (i < 0) return;
    this.start = i;
    this.#paintMoving();
  }

  override onMouseMove(pt: Point): void {
    this.#pointer = pt;
  }
  override onMouseLeave(): void {
    this.#pointer = null;
  }

  /** `OnSetCursor`: the select cursor over a start cell until the start. */
  uiCursor(): number | null {
    return !this.started && this.#pointer !== null && this.startAt(this.#pointer) >= 0
      ? CUR_SELECT
      : null;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.started = true;
      this.okButton?.enableWindow(false);
      this.#request.roll();
      this.#paint();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  // ---- the run -----------------------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    super.process(now);
    const frames = this.#clock.advance(now);
    for (let i = 0; i < frames; i += 1) {
      this.#pulse.tick();
      if (this.run && !this.#ended && this.run.step() && this.#finishRun()) break;
    }
    if (frames > 0) this.#paintMoving();
  }

  /** The ball is at the bottom: FINISH ends the game (`true`: the window closes), FAIL shows the box once. */
  #finishRun(): boolean {
    if (this.resultState === MP.FINISH) {
      this.#ended = true;
      this.#paintMoving();
      this.#request.end();
      this.destroy();
      return true;
    }
    if (this.resultState === MP.FAIL && !this.#failShown) {
      this.#failShown = true;
      this.#deps.openFail(() => {
        this.reset();
      });
    }
    return false;
  }
}
