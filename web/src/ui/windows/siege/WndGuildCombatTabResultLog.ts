// `WndGuildCombatTabResultLog` (`CWndGuildCombatTabResultLog`, `src:_Interface/WndField.cpp:18201-18268`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-51): `APP_GUILDCOMBAT_TAB_RESULT_LOG` (571,
// 528 x 256), the second page of the result window: the kill log in `WIDC_TEXT1` and a save button (`BUTTON1`). Save hands the PLAIN text of the log to the request, which downloads it as
// `GuildLog_YYYYMMDD.txt` from the click handler (a browser needs the user's gesture), and the page answers with the box "The log was saved as <file>" (`TID_GAME_GUILDCOMBAT_LOG_SAVE`).

import { WIDC_BUTTON1, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import { TID_GAME_GUILDCOMBAT_LOG_SAVE } from '../../../gen/defs/text';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { CodePage } from '../char/CodePage';
import type { SiegeHost } from './ports';

export interface TabResultLogDeps {
  readonly host: SiegeHost;
  /** The plain text goes to a file; the answer is its name, `''` when nothing was saved. */
  save(plainText: string): string;
}

export class WndGuildCombatTabResultLog extends CodePage {
  readonly #deps: TabResultLogDeps;

  constructor(deps: TabResultLogDeps) {
    super();
    this.#deps = deps;
  }

  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  /** `ResetString` and the lines of `InsertLog` (markup: `#cff009c00 ... #nc`). */
  setLog(markup: string): void {
    const t = this.textCtrl;
    if (!t) return;
    t.setString('');
    t.setString(markup);
  }

  /** `WIDC_BUTTON1`. */
  save(): void {
    const plain = this.textCtrl?.getString() ?? '';
    const file = this.#deps.save(plain);
    if (file === '') return;
    this.#deps.host.box(this.#deps.host.fmt(TID_GAME_GUILDCOMBAT_LOG_SAVE, file));
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.save();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
