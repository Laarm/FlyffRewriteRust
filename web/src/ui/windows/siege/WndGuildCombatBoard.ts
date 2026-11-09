// `WndGuildCombatBoard` (`CWndGuildCombatBoard( nCombatType )`, `src:_Interface/WndField.cpp:16126-16235`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.0; WP-UI-49): `APP_GUILDCOMBAT_BOARD` (559, 384 x 384), the
// read-only information board of the siege NPC: one panel of the lazy help bundle (`GuildCombatTEXT_<n>_ENG.inc`) in `WIDC_TEXT1`, `BUTTON1` closes. The title is that of the mode
// (`TID_GAME_GUILDCOMBAT_BOARD` / `_1TO1_BOARD`, set by `PaintFrame` in the original). A missing panel opens nothing (the caller checks).

import { APP_GUILDCOMBAT_BOARD, WIDC_BUTTON1, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT_1TO1_BOARD,
  TID_GAME_GUILDCOMBAT_BOARD,
} from '../../../gen/defs/text';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { BoardRequest, SiegeHost } from './ports';

export interface BoardDeps {
  readonly host: SiegeHost;
}

export class WndGuildCombatBoard extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_BOARD;
  readonly #request: BoardRequest;
  readonly #host: SiegeHost;

  constructor(request: BoardRequest, deps: BoardDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get mode(): 0 | 1 {
    return this.#request.mode;
  }
  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.setTitle(
      this.#host.text(
        this.#request.mode === 0 ? TID_GAME_GUILDCOMBAT_BOARD : TID_GAME_GUILDCOMBAT_1TO1_BOARD,
      ),
    );
    this.textCtrl?.setString(this.#request.text);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
