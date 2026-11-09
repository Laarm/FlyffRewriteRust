// `WndLordInfo` (`CWndLordInfo`, `src:_Interface/WndLord.cpp:1041-1148`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-55): `APP_LORD_INFO` (756, 304 x 320, "Lord info Window"). `TEXT1` shows the panel
// `lordinfo` of the help bundle; `STATIC4` the state of the election (Ready: a lord exists "Lord exists" / "Lord does not exist"; candidacy and vote their period; Expired "Lord does not exist"), `STATIC5`
// the name of the lord and `STATIC6` the time left until the vote of the next election ends ("%d Day(s) %d Hr(s) %d Min(s) %d Sec(s)"): both only while a lord exists, the time only while it is not
// negative. All three are recomputed at every process (the original did it in `OnDraw`); FIX: the time counts against the SERVER clock (`time_null()` read the PC clock). `BUTTON1` closes.

import {
  APP_LORD_INFO,
  WIDC_BUTTON1,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { TID_TOOLTIP_DATE } from '../../../gen/defs/text';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LordWindowDeps } from './ports';
import { restTime, statusTid } from './rows';

export class WndLordInfo extends WndNeuz {
  readonly appId = APP_LORD_INFO;
  readonly #deps: LordWindowDeps;

  constructor(deps: LordWindowDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get infoText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get statusStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC4);
  }
  get nameStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC5);
  }
  get timeStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC6);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const text = this.#deps.ports.panel('lordinfo');
    if (text !== '') this.infoText?.setString(text);
    this.moveParentCenter();
    this.#refresh();
  }

  /** `OnDraw`: the status, the name and the time. */
  #refresh(): void {
    const { ports, host } = this.#deps;
    const view = ports.view.get();
    const hasLord = view.lordId !== null;
    if (hasLord) {
      this.nameStatic?.setTitle(view.lordName);
      const rest = view.endVoteAt - ports.nowS();
      if (view.endVoteAt !== 0 && rest >= 0) {
        const t = restTime(rest);
        this.timeStatic?.setTitle(
          host.fmt(TID_TOOLTIP_DATE, t.days, t.hours, t.minutes, t.seconds),
        );
      } else {
        this.timeStatic?.setTitle('');
      }
    } else {
      this.nameStatic?.setTitle('');
      this.timeStatic?.setTitle('');
    }
    this.statusStatic?.setTitle(host.text(statusTid(view.state, hasLord)));
  }

  override process(now: number): void {
    this.#refresh();
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
