// `WndGuildCombatJoinSelection` (`CWndGuildCombatJoinSelection`, `src:_Interface/WndField.cpp:17141-17290`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-49): `APP_GUILDCOMBAT_SELSTART` (557, 320 x 176),
// the gate choice of a fighter before the war starts. Four gate buttons (`BUTTON1..4`) send `SelectMapGuildCombat{map 0..3}` at once and frame the chosen gate in blue (`0xFF0000FF`, the button
// rect deflated by one); `STATIC2` reads "N seconds left, gate G" with `G` = "Random" until a gate is clicked (the stored gate, `99`, is random). When the time is up the window sends the
// current choice and closes by itself. It has no title bar and the Esc chain does not close it (`ESC_SKIP_APPS`).

import { APP_GUILDCOMBAT_SELSTART, WIDC_STATIC2 } from '../../../gen/defs/resdata';
import { TID_GAME_GUILDCOMBAT_JOIN_MESSAGE } from '../../../gen/defs/text';
import { RectLayer } from '../../controls/RectLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { GATE_RANDOM, type SelStartRequest, type SiegeHost } from './ports';
import { GATE_BUTTONS, GATE_FRAME_COLOR } from './rows';

export interface SelStartDeps {
  readonly host: SiegeHost;
}

const ID_FRAME = 9000;

export class WndGuildCombatJoinSelection extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_SELSTART;
  readonly #request: SelStartRequest;
  readonly #host: SiegeHost;
  /** `m_nMapNum`. */
  #map: number;
  /** `m_dwOldTime`: when the choice ends, in the clock of the UI (ms). */
  #endMs = 0;
  #text: Static | null = null;
  #frame: RectLayer | null = null;
  #shown = '';
  #done = false;

  constructor(request: SelStartRequest, deps: SelStartDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.#map = request.map;
    this.putRegInfo = false;
  }

  /** The gate chosen now (`99` = random). */
  get map(): number {
    return this.#map;
  }
  get messageStatic(): Static | null {
    return this.#text;
  }
  get frameLayer(): RectLayer | null {
    return this.#frame;
  }
  /** The ms left until the choice ends (never negative). */
  leftMs(now: number = this.env.host.now()): number {
    return Math.max(0, this.#endMs - now);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.titleBar?.setVisible(false);
    this.#text = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#endMs = this.env.host.now() + this.#request.timeoutMs;
    const rect = this.rectClient;
    this.#frame = this.addControl(
      new RectLayer(),
      ID_FRAME,
      { l: 0, t: 0, r: rectWidth(rect), b: rectHeight(rect) },
      WBS_NOFRAME,
    );
    this.#refresh(this.env.host.now());
    this.moveParentCenter();
  }

  /** The message and the frame (`OnDraw`). */
  #refresh(now: number): void {
    const left = Math.floor(this.leftMs(now) / 1000) + 1;
    const gate = this.#map === GATE_RANDOM ? 'Random' : String(this.#map + 1);
    const text = this.#host.fmt(TID_GAME_GUILDCOMBAT_JOIN_MESSAGE, left, gate);
    if (text !== this.#shown) {
      this.#shown = text;
      this.#text?.setTitle(text);
    }
    const id = GATE_BUTTONS[this.#map];
    const at = id === undefined ? null : this.getWndCtrl(id)?.rect;
    this.#frame?.setEntries(
      at
        ? [
            {
              key: 'gate',
              x: at.l + 1,
              y: at.t + 1,
              w: rectWidth(at) - 2,
              h: rectHeight(at) - 2,
              outline: { color: GATE_FRAME_COLOR, width: 1 },
            },
          ]
        : [],
    );
  }

  /** A gate button: the choice goes out at once. */
  choose(map: number): void {
    this.#request.select(map);
    this.#map = map;
    this.#refresh(this.env.host.now());
  }

  override process(now: number): void {
    if (!this.#done && this.#endMs <= now) {
      // `Process`: the time is up, the current choice goes out and the window closes
      this.#done = true;
      this.#request.select(this.#map);
      this.destroy();
      return;
    }
    this.#refresh(now);
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const gate = GATE_BUTTONS.indexOf(id as (typeof GATE_BUTTONS)[number]);
      if (gate >= 0) {
        this.choose(gate);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
