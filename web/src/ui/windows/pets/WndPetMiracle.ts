// `WndPetMiracle` (`CWndPetMiracle`, `src:_Interface/WndPetSys.cpp:1268-1632`, ref/22 §17 M, Plan/42 §19.4 M, 45 §18.7; WP-UI-33): `APP_PET_MIRACLE` (272 x 336,
// "Chance of the Pet Tamer"), the window of the tamer's chance scroll. It compares the stat picks (1..9, drawn as the large `Icon_PetLevel<n>_L.dds`) of the
// pet's last two levels (`WIDC_STATIC1/2` carry the level names, `WIDC_STATIC3/4` the pictures); only a B / A / S pet qualifies, any other pet or no pet closes
// the window at once. "Chance" (`WIDC_BUTTON2`) sends `PetTamerMiracle` once and disables itself; the answer (`Pet` with `PF_PET_GET_AVAIL`) arrives as
// `receiveResult(pre, cur)` and starts the spin:
//   * both pictures show random picks (never the same twice in a row, `rand() % 9`), one change every 4th frame, with `InfOpen.wav` at each (T39): the left
//     one for 60 frames, the right one for 80;
//   * then they slow down - the wait between two changes grows by one each time - until it reaches 10 (left) or 20 (right) and the true pick is shown;
//   * when the right picture has settled the Chance button goes and OK (`WIDC_OK`, hidden until then) comes.
// The status window is told to keep showing the old picks meanwhile (`LockShowLevel`); closing the window lets them show again. `process` is the per-frame
// `Process()` of the original (one call = one frame).

import {
  APP_PET_MIRACLE,
  WIDC_BUTTON2,
  WIDC_CLOSE,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { TextLayer } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import type { Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PetMiracleRequest } from './ports';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_SPRITES = 0x7a50;
const ID_TEXTS = 0x7a51;
/** The first phase lasts this many frames (left, right); `m_nCount > 60` / `> 80`. */
export const SPIN_FRAMES = [60, 80] as const;
/** The wait between two changes grows to this (left, right) before the true pick is shown. */
export const SLOW_LIMIT = [10, 20] as const;

type Side = 0 | 1;

interface SideState {
  /** `m_nCount`. */
  count: number;
  /** `m_nDelay`. */
  delay: number;
  /** `m_nStatus`: -1 idle, 0 spinning, 1 slowing. */
  status: -1 | 0 | 1;
  /** `m_nPreLvCount` / `m_nCurLvCount`: the shown index (0..8). */
  shown: number;
  /** `m_nMiracleLv`: the pick (1..9) of the side. */
  pick: number;
  /** `m_bReciveResult`. */
  received: boolean;
  /** `m_bLocked`. */
  locked: boolean;
}

export class WndPetMiracle extends WndNeuz {
  readonly appId = APP_PET_MIRACLE;
  readonly #request: PetMiracleRequest;
  readonly #side: [SideState, SideState];
  #rects: [Rect, Rect] = [NO_RECT, NO_RECT];
  #pics: [Rect, Rect] = [NO_RECT, NO_RECT];
  #sprites: SpriteLayer | null = null;
  #texts: TextLayer | null = null;
  #end = false;
  #sent = false;

  constructor(request: PetMiracleRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
    const mk = (pick: number): SideState => ({
      count: 0,
      delay: 1,
      status: -1,
      shown: pick - 1,
      pick,
      received: false,
      locked: false,
    });
    const p = request.picks;
    this.#side = [mk(p?.[0] ?? 1), mk(p?.[1] ?? 1)];
  }

  /** The pick of a side (1..9), `m_nMiracleLv`. */
  pickOf(side: Side): number {
    return this.#side[side].pick;
  }
  /** The picture index (0..8) a side shows right now. */
  shownIndex(side: Side): number {
    const s = this.#side[side];
    return s.received ? s.shown : s.pick - 1;
  }
  /** `true` once a result came and both sides have settled (the original's `m_bEnd` is set by either side). */
  get finished(): boolean {
    return this.#end && this.#side[0].status === -1 && this.#side[1].status === -1;
  }
  get chanceButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get description(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    if (this.#request.picks === null) {
      // not a B / A / S pet, or no pet at all
      this.destroy();
      return;
    }
    const rectOf = (id: number): Rect => this.getWndCtrl(id)?.rect ?? NO_RECT;
    this.#rects = [rectOf(WIDC_STATIC1), rectOf(WIDC_STATIC2)];
    this.#pics = [rectOf(WIDC_STATIC3), rectOf(WIDC_STATIC4)];
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.description?.setString(this.#request.description);
    const ok = this.okButton;
    ok?.enableWindow(false);
    ok?.setVisible(false);
    // the status window keeps the old picks while the roll runs
    this.#request.lock(true, 0);
    this.#request.lock(true, 1);
    this.#side[0].locked = true;
    this.#side[1].locked = true;
    this.moveParentCenter();
    this.#paint();
  }

  // ---- the roll ----------------------------------------------------------------------------------------------------------------

  /** `ReceiveResult( nPreLevel, nCurLevel )`: the new picks arrived; both sides start to spin. */
  receiveResult(pre: number, cur: number): void {
    for (const [i, pick] of [pre, cur].entries()) {
      const s = this.#side[i as Side];
      s.pick = pick;
      s.status = 0;
      s.received = true;
      // the original keeps the result in `m_nRes*Level` and shows it at the end
    }
    this.#result = [pre, cur];
  }
  #result: [number, number] = [0, 0];

  /** `rand() % 9` that differs from `current` (the next index, wrapping, when it hits). */
  #next(current: number): number {
    let n = this.#request.random(9);
    if (n === current) {
      n += 1;
      if (n > 8) n = 0;
    }
    return n;
  }

  /** `PreLevelImgProcess` / `CurLevelImgProcess`: one frame of one side. */
  #frame(side: Side): void {
    const s = this.#side[side];
    const spin = SPIN_FRAMES[side];
    const slow = SLOW_LIMIT[side];
    if (s.status === 0) {
      if (s.count % 4 === 0) {
        if (s.count > spin) {
          s.count = 0;
          s.status = 1;
        } else {
          this.#request.tick();
          s.shown = this.#next(s.shown);
        }
      }
      s.count += 1;
    } else if (s.status === 1) {
      if (s.count > s.delay) {
        if (s.delay < slow) {
          this.#request.tick();
          s.delay += 1;
        } else {
          this.#request.tick();
          // the roll ends: the true pick shows, the status window may show it too
          s.shown = this.#result[side] - 1;
          s.status = -1;
          s.delay = 1;
          s.count = 0;
          s.received = false;
          this.#end = true;
          if (s.locked) {
            this.#request.lock(false, side);
            s.locked = false;
          }
          if (side === 1) {
            this.chanceButton?.setVisible(false);
            const ok = this.okButton;
            ok?.setVisible(true);
            ok?.enableWindow(true);
          }
        }
        s.shown = s.status === -1 ? s.shown : this.#next(s.shown);
        s.count = 0;
      }
      s.count += 1;
    }
  }

  override process(now: number): void {
    if (this.#side[0].status !== -1 || this.#side[1].status !== -1) {
      this.#frame(0);
      this.#frame(1);
      this.#paint();
    }
    super.process(now);
  }

  #paint(): void {
    if (!this.#sprites || !this.#texts) return;
    const names = this.#request.levelNames;
    this.#texts.setItems([
      {
        x: this.#rects[0].l + 8,
        y: this.#rects[0].t + 3,
        text: names[0],
        color: 0xffffffff,
        font: 'title',
      },
      {
        x: this.#rects[1].l + 8,
        y: this.#rects[1].t + 3,
        text: names[1],
        color: 0xffffffff,
        font: 'title',
      },
    ]);
    this.#sprites.setEntries(
      ([0, 1] as const).map((side) => ({
        key: `pick${String(side)}`,
        path: this.#request.pickIcon(this.shownIndex(side) + 1),
        x: this.#pics[side].l,
        y: this.#pics[side].t + 4,
        w: 90,
        h: 90,
      })),
    );
  }

  // ---- buttons -----------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON2) {
        this.chanceButton?.enableWindow(false);
        if (!this.#sent) {
          this.#sent = true;
          this.#request.send(this.#request.scroll);
        }
        return true;
      }
      if (id === WIDC_OK || id === WIDC_CLOSE) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    // `OnDestroy`: the locks of the status window are lifted
    for (const side of [0, 1] as const) {
      const s = this.#side[side];
      if (s.locked) {
        this.#request.lock(false, side);
        s.locked = false;
      }
    }
    this.#request.closed?.();
    super.onDestroy();
  }
}
