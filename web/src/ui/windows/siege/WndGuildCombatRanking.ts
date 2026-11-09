// `WndGuildCombatRanking` (`CWndGuildCombatRanking`, `src:_Interface/WndField.cpp:17814-18010`, ref/22 §17 K, Plan/42 §19.4 K; WP-UI-50): `APP_GUILDCOMBAT_RANKING` (521, 256 x 272), the ranking of the
// guilds by wins. A SHELL: V15 never instantiates the class (`reachable: false`), the fixture and the tests build it. The statics of the layout are the header; the rows are drawn by hand
// like the person ranking: the rank number (three characters, blank for a guild with the wins of the one above), the guild (cut at 8 characters and dots) and the count, the first row red.
// The guilds are ordered by wins, highest first, equal counts in reverse order of insertion (the original's multimap read from `rbegin()`).

import { APP_GUILDCOMBAT_RANKING } from '../../../gen/defs/resdata';
import { WndNeuz } from '../../wnd/WndNeuz';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME } from '../../layout/styles';
import { rankingRows } from './rows';
import type { RankingRequest } from './ports';
import { TableList, type TableSpec } from './TableList';

const SPEC: TableSpec = { left: 8, ruleWidth: 230, selectWidth: 215, page: 11 };
const ID_TABLE = 0x7f51;

export class WndGuildCombatRanking extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_RANKING;
  readonly #request: RankingRequest;
  #table: TableList | null = null;

  constructor(request: RankingRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get table(): TableList | null {
    return this.#table;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rect = this.rectClient;
    const table = new TableList(SPEC);
    this.#table = this.addControl(
      table,
      ID_TABLE,
      { l: 0, t: 0, r: rectWidth(rect), b: rectHeight(rect) },
      WBS_NOFRAME,
    );
    table.setRows(rankingRows(this.#request.guilds));
    this.moveParentCenter();
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
