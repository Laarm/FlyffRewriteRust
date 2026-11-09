// `GuildPowerPage` (`CWndGuildTabPower`, `src:_Interface/WndGuildTabPower.cpp`, `__GUILD_HOUSE`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-46): the fifth tab of `APP_GUILD`
// ("Agit", `APP_GUILD_TAPPOWER`, 404 x 304), shown as in the original's `__GUILD_HOUSE` build. Two rows of check boxes for the kingpin, captain, supporter and rookie: the
// furniture right (`PF_GUILDHOUSE_FURNITURE` 0x20, `WIDC_CHECK1..4`) and the upkeep right (`PF_GUILDHOUSE_UPKEEP` 0x40, `WIDC_CHECK9..12`); `WIDC_BUTTON1` sends, enabled only after
// a change; `WIDC_BUTTON11` (cancel) stays locked as in the original. The guild house itself (its effects) is M7.
//
// FIX: the page shares the power draft with the appellation page (`PowerView`, one copy: the original kept two that each sent all five words), shows it both ways and is
// read-only for everybody but the master.

import { WIDC_BUTTON1, WIDC_BUTTON11 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { CodePage } from '../char/CodePage';
import { type GuildActions, PF, type PowerView } from './ports';

/** The check boxes of the page: layout name -> (member level, bit). Kingpin = 1 .. rookie = 4. */
export const POWER_CHECKS: readonly { name: string; rank: number; bit: number }[] = [
  { name: 'WIDC_CHECK1', rank: 1, bit: PF.GH_FURNITURE },
  { name: 'WIDC_CHECK2', rank: 2, bit: PF.GH_FURNITURE },
  { name: 'WIDC_CHECK3', rank: 3, bit: PF.GH_FURNITURE },
  { name: 'WIDC_CHECK4', rank: 4, bit: PF.GH_FURNITURE },
  { name: 'WIDC_CHECK9', rank: 1, bit: PF.GH_UPKEEP },
  { name: 'WIDC_CHECK10', rank: 2, bit: PF.GH_UPKEEP },
  { name: 'WIDC_CHECK11', rank: 3, bit: PF.GH_UPKEEP },
  { name: 'WIDC_CHECK12', rank: 4, bit: PF.GH_UPKEEP },
];

export class GuildPowerPage extends CodePage {
  readonly #actions: GuildActions;
  readonly #byId = new Map<number, { rank: number; bit: number }>();
  #power: PowerView | null = null;

  constructor(actions: GuildActions) {
    super();
    this.#actions = actions;
  }

  /** The check box of a member level (1..4) and bit. */
  check(rank: number, bit: number): Button | null {
    const row = POWER_CHECKS.find((c) => c.rank === rank && c.bit === bit);
    return row ? this.getDlgItemByName<Button>(row.name) : null;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON11);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    for (const row of POWER_CHECKS) {
      const c = this.getDlgItemByName<Button>(row.name);
      if (c) this.#byId.set(c.id, { rank: row.rank, bit: row.bit });
    }
    if (this.#power) this.applyPower(this.#power);
    else this.okButton?.enableWindow(false);
    this.cancelButton?.enableWindow(false);
  }

  /** `SetData` + `EnableButton`: the boxes show the draft, only the master may change it, the button waits for a change. */
  applyPower(p: PowerView): void {
    this.#power = p;
    for (const row of POWER_CHECKS) {
      const c = this.getDlgItemByName<Button>(row.name);
      if (!c) continue;
      c.setCheck(((p.words[row.rank] ?? 0) & row.bit) !== 0);
      c.enableWindow(p.editable);
    }
    this.okButton?.enableWindow(p.editable && p.dirty);
    this.cancelButton?.enableWindow(false);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const box = this.#byId.get(id);
      if (box) {
        const c = this.getDlgItem<Button>(id);
        if (c) this.#actions.setPower(box.rank, box.bit, c.getCheck() !== 0);
        return true;
      }
      if (id === WIDC_BUTTON1) {
        this.#actions.sendPower();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
