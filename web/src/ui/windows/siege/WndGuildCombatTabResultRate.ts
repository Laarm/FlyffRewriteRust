// `WndGuildCombatTabResultRate` (`CWndGuildCombatTabResultRate`, `src:_Interface/WndField.cpp:18159-18200`, ref/22 §17 K, Plan/42 §19.4 K; WP-UI-51): `APP_GUILDCOMBAT_TAB_RESULT` (569, 528 x 240), the
// first page of the result window: the guild ranking in `WIDC_LISTBOX1` and the member ranking in `WIDC_LISTBOX2`. The rows are text lines the result window builds from the stored
// precedence lists when it opens.

import { WIDC_LISTBOX1, WIDC_LISTBOX2 } from '../../../gen/defs/resdata';
import type { ListBox } from '../../controls/ListBox';
import { CodePage } from '../char/CodePage';

export class WndGuildCombatTabResultRate extends CodePage {
  get guildList(): ListBox | null {
    return this.getDlgItem<ListBox>(WIDC_LISTBOX1);
  }
  get personList(): ListBox | null {
    return this.getDlgItem<ListBox>(WIDC_LISTBOX2);
  }

  /** `ResetContent` of both lists, then `InsertGuildRate` / `InsertPersonRate` per row. */
  setRows(guildRows: readonly string[], personRows: readonly string[]): void {
    const g = this.guildList;
    const p = this.personList;
    g?.resetContent();
    p?.resetContent();
    for (const r of guildRows) g?.addString(r);
    for (const r of personRows) p?.addString(r);
  }
}
