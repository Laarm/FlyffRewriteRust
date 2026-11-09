// `WndGuildCombatResult` (`CWndGuildCombatResult`, `src:_Interface/WndField.cpp:18075-18160`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-50): `APP_GUILDCOMBAT_RESULT_POINT` (567, 560 x 320), the
// result window of the siege (`GC_LOG`, opened even with an empty log). One tab control (`WIDC_TABCTRL1`) with two pages: the guild and member rankings (`WndGuildCombatTabResultRate`, caption
// `TID_GAME_TOOLTIP_LOG1`) and the kill log (`WndGuildCombatTabResultLog`, `TID_GAME_TOOLTIP_LOG2`). The rows come from the request, built from the stored precedence slices when the
// window opens. The original's `SetButtonLength( 100 )` of the tab clicks is not ported (a tab answers a click on its drawn width).

import {
  APP_GUILDCOMBAT_RESULT_POINT,
  APP_GUILDCOMBAT_TAB_RESULT,
  APP_GUILDCOMBAT_TAB_RESULT_LOG,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import { TID_GAME_TOOLTIP_LOG1, TID_GAME_TOOLTIP_LOG2 } from '../../../gen/defs/text';
import type { TabCtrl } from '../../controls/TabCtrl';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { ResultRequest, SiegeHost } from './ports';
import { WndGuildCombatTabResultLog } from './WndGuildCombatTabResultLog';
import { WndGuildCombatTabResultRate } from './WndGuildCombatTabResultRate';

export interface ResultDeps {
  readonly host: SiegeHost;
}

export class WndGuildCombatResult extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_RESULT_POINT;
  readonly #request: ResultRequest;
  readonly #host: SiegeHost;
  #tab: TabCtrl | null = null;
  #rate: WndGuildCombatTabResultRate | null = null;
  #log: WndGuildCombatTabResultLog | null = null;

  constructor(request: ResultRequest, deps: ResultDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get ratePage(): WndGuildCombatTabResultRate | null {
    return this.#rate;
  }
  get logPage(): WndGuildCombatTabResultLog | null {
    return this.#log;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      this.#tab = tab;
      const rate = new WndGuildCombatTabResultRate();
      rate.attach(tab, APP_GUILDCOMBAT_TAB_RESULT);
      const log = new WndGuildCombatTabResultLog({
        host: this.#host,
        save: (plain) => this.#request.save(plain),
      });
      log.attach(tab, APP_GUILDCOMBAT_TAB_RESULT_LOG);
      this.#rate = rate;
      this.#log = log;
      tab.insertItem(0, { text: this.#host.text(TID_GAME_TOOLTIP_LOG1), page: rate });
      tab.insertItem(1, { text: this.#host.text(TID_GAME_TOOLTIP_LOG2), page: log });
      // `ResetContent` and the `Insert*` calls of `OnGCLog`
      rate.setRows(this.#request.guildRows, this.#request.personRows);
      log.setLog(this.#request.log);
    }
    this.moveParentCenter();
  }

  /** New rows after a name arrived: the lists and the log are replaced. */
  update(rows: Pick<ResultRequest, 'guildRows' | 'personRows' | 'log'>): void {
    this.#rate?.setRows(rows.guildRows, rows.personRows);
    this.#log?.setLog(rows.log);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
