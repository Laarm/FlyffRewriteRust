// `WndGuildWarState` (`CWndGuildWarState`, `src:_Interface/WndField.cpp:17600-17815`, ref/22 §17 K, Plan/42 §19.4 K; WP-UI-51): `APP_GUILD_WAR_STATE` (519, 336 x 272), the entry list of the guilds
// of a combat. A SHELL: V15 never instantiates the class (`reachable: false`). The statics of the layout are the header; the rows are drawn by hand: the number (`%3d`), the guild (cut at 8
// characters and dots), the master's name and the member count (`%2d`), eleven per page with a scroll bar. An entry with no members is dropped (`InsertGuild` returned for `nNum == 0`).

import { APP_GUILD_WAR_STATE } from '../../../gen/defs/resdata';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME } from '../../layout/styles';
import { WndNeuz } from '../../wnd/WndNeuz';
import { warStateRows } from './rows';
import type { WarStateRequest } from './ports';
import { TableList, type TableSpec } from './TableList';

const SPEC: TableSpec = { left: 8, ruleWidth: 310, selectWidth: 295, page: 11 };
const ID_TABLE = 0x7f52;

export class WndGuildWarState extends WndNeuz {
  readonly appId = APP_GUILD_WAR_STATE;
  readonly #request: WarStateRequest;
  #table: TableList | null = null;

  constructor(request: WarStateRequest) {
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
    table.setRows(warStateRows(this.#request.entries));
    this.moveParentCenter();
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
