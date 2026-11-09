// `WndSecretRoomGuildMemMng` (`CWndSecretRoomGuildMemMng`, `src:_Interface/WndSecretRoom.cpp:1286-1835`, ref/22 §17 K, Plan/42 §19.4 K; WP-UI-57): `APP_SECRETROOM_GUILDMEM_MNG` (751, 144 x 272), the
// member list of the old quick window. V15 comments the class out (no opener): this is a SHELL over the real layout for CA1 / CA2 (`reachable: false`). `LISTBOX1` has one row "%d %s" per
// member (the number and the name, cut over 14 characters).

import { APP_SECRETROOM_GUILDMEM_MNG, WIDC_LISTBOX1 } from '../../../gen/defs/resdata';
import type { ListBox } from '../../controls/ListBox';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { GuildMemMngRequest } from './ports';
import { cutOver } from './rows';

export class WndSecretRoomGuildMemMng extends WndNeuz {
  readonly appId = APP_SECRETROOM_GUILDMEM_MNG;
  readonly #request: GuildMemMngRequest;

  constructor(request: GuildMemMngRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get list(): ListBox | null {
    return this.getDlgItem<ListBox>(WIDC_LISTBOX1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#request.members.forEach((m, i) => {
      this.list?.addString(cutOver(`${String(i + 1)} ${m.name}`, 14));
    });
    this.moveParentCenter();
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
