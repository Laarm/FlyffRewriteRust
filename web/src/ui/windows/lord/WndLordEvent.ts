// `WndLordEvent` (`CWndLordEvent`, `src:_Interface/WndLord.cpp:618-746`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-54): `APP_LORD_EVENT` (726, 320 x 208, "Lord's Event Window"), the lord's event: an
// experience rate and an item drop rate picked from the two lists of `lordevent.inc` (11 rows each, row 0 is 0 %) and their price. The indices start at 0 / 0; `BUTTON1` / `BUTTON2` step the experience
// row down / up and `BUTTON3` / `BUTTON4` the item row, wrapping at both ends (FIX: the original wrapped `BUTTON4` over the experience size; the tables have the same length). `BUTTON6` lies outside the
// window (y -208 in the layout) and has no handler: built, never reachable. The texts are redrawn from the indices (`OnDraw`): "  %d %" for each rate with the original's hack (`nTemp % 5 != 0`
// adds one: 0.35f -> 34 -> 35), the price as the comma form of the sum of both rows (0 for 0 / 0). `BUTTON5` starts the event: with `features.player_lordevent` (`__PLAYER_LORDEVENT`) as long as fewer than
// `gameplay.max_lord_event` events run, else it prints the hard-coded line; without the feature only while the own character has no event. A character that is not the lord cannot open the window
// without `player_lordevent` (the chat line `TID_GAME_L_EVENT_CREATE_E001`).

import {
  APP_LORD_EVENT,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_BUTTON4,
  WIDC_BUTTON5,
  WIDC_TEXT1,
  WIDC_TEXT2,
  WIDC_TEXT3,
} from '../../../gen/defs/resdata';
import { TID_GAME_L_EVENT_CREATE_E001 } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { type LordView, type LordWindowDeps, MAX_LORD_EVENT_TEXT } from './ports';
import { commaCost, eventCost, percentText, stepIndex } from './rows';

export class WndLordEvent extends WndNeuz {
  readonly appId = APP_LORD_EVENT;
  readonly #deps: LordWindowDeps;
  /** `m_nEEvent`, `m_nDEvent`. */
  #e = 0;
  #d = 0;
  #shown = '';

  constructor(deps: LordWindowDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get expIndex(): number {
    return this.#e;
  }
  get itemIndex(): number {
    return this.#d;
  }
  get expText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get itemText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT2);
  }
  get costText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT3);
  }
  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON5);
  }
  /** The unreachable sixth button of the layout (y -208). */
  get outsideButton(): Button | null {
    return this.getDlgItemByName<Button>('WIDC_BUTTON6');
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const view = this.#deps.ports.view.get();
    // `Initialize`: without `__PLAYER_LORDEVENT` only the lord may use the window
    if (!view.playerLordEvent && !view.isLord) {
      this.#deps.ports.say(TID_GAME_L_EVENT_CREATE_E001);
      this.destroy();
      return;
    }
    this.moveParentCenter();
    this.#draw();
  }

  /** `OnDraw`. */
  #draw(): void {
    const v = this.#deps.ports.view.get();
    const e = v.expRates[this.#e]?.factor ?? 1;
    const i = v.itemRates[this.#d]?.factor ?? 1;
    const exp = percentText(e);
    const item = percentText(i);
    const cost = commaCost(eventCost(v.expRates, v.itemRates, this.#e, this.#d));
    const key = `${exp}|${item}|${cost}`;
    if (key === this.#shown) return;
    this.#shown = key;
    this.expText?.setString(exp);
    this.itemText?.setString(item);
    this.costText?.setString(cost);
  }

  override process(now: number): void {
    this.#draw();
    super.process(now);
  }

  /** One of the four step buttons. */
  step(button: 'expDown' | 'expUp' | 'itemDown' | 'itemUp'): void {
    const v = this.#deps.ports.view.get();
    switch (button) {
      case 'expDown':
        this.#e = stepIndex(this.#e, -1, v.expRates.length);
        break;
      case 'expUp':
        this.#e = stepIndex(this.#e, 1, v.expRates.length);
        break;
      case 'itemDown':
        this.#d = stepIndex(this.#d, -1, v.itemRates.length);
        break;
      case 'itemUp':
        this.#d = stepIndex(this.#d, 1, v.itemRates.length);
        break;
    }
    this.#draw();
  }

  /** `BUTTON5`; returns `true` when `SendLEventCreate` was called. */
  start(): boolean {
    const { ports } = this.#deps;
    const v: LordView = ports.view.get();
    if (v.playerLordEvent) {
      if (v.eventCount >= v.maxLordEvent) {
        ports.sayText(MAX_LORD_EVENT_TEXT);
        return false;
      }
      ports.actions.createEvent(this.#e, this.#d);
      return true;
    }
    if (!v.ownEvent) {
      ports.actions.createEvent(this.#e, this.#d);
      return true;
    }
    return false;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_BUTTON1:
          this.step('expDown');
          return true;
        case WIDC_BUTTON2:
          this.step('expUp');
          return true;
        case WIDC_BUTTON3:
          this.step('itemDown');
          return true;
        case WIDC_BUTTON4:
          this.step('itemUp');
          return true;
        case WIDC_BUTTON5:
          this.start();
          return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
