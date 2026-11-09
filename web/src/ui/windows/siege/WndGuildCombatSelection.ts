// `WndGuildCombatSelection` (`CWndGuildCombatSelection`, `src:_Interface/WndField.cpp:16380-17000`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-49): `APP_GUILDCOMBAT_SELECTION` (551, 656 x 432), the
// line-up editor of the siege. `LISTBOX1` lists the online guild members, ascending by level; `LISTBOX2` is the line-up in fight order, the first `maxWar` rows under a blue band (the fighters)
// and the rest under a red one (the waiting players); `BUTTON1` / `BUTTON2` move a member across, `BUTTON3` / `BUTTON4` move a row up or down, `BUTTON7` makes the selected row the defender (the
// "D" button `BUTTON8` marks him), `RESET` asks to start again and `FINISH` sends the line-up.
//
// Rules of the original with the FIXES the task names: the add button refuses when `maxJoin` players are in (the original allowed an 11th: `maxJoin < size`), the level check needs 30, a player
// that is no guild member gets a box (the original dereferenced a missing member and crashed), the master may not be the defender of a line-up of more than one, FINISH checks the defender first,
// then the war state (the window closes when the line-up phase is over), the player count and the master / kingpin. The window opens only while the war state is 101; the pre-selected
// players of an earlier submission stay in the left list as well (KEEP: adding one of them says "already entered"). The "D" button hides when no defender is set (FIX: the original compared
// an unsigned -1 and showed it at the end of the list).

import {
  APP_GUILDCOMBAT_SELECTION,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_BUTTON4,
  WIDC_BUTTON7,
  WIDC_BUTTON8,
  WIDC_CLOSE,
  WIDC_FINISH,
  WIDC_LISTBOX1,
  WIDC_LISTBOX2,
  WIDC_RESET,
  WIDC_STATIC3,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT_ALREADY_ENTRY,
  TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP,
  TID_GAME_GUILDCOMBAT_HAVENOT_MASTER,
  TID_GAME_GUILDCOMBAT_HAVENOT_PLAYER,
  TID_GAME_GUILDCOMBAT_LIMIT_LEVEL_NOTICE,
  TID_GAME_GUILDCOMBAT_MASTER_NOT_ASSIGN_DEFENDER,
  TID_GAME_GUILDCOMBAT_NOT_ASSIGN_DEFENDER,
  TID_GAME_GUILDCOMBAT_NOT_GUILD_MEMBER,
  TID_GAME_GUILDCOMBAT_OFFER_INFO,
  TID_GAME_GUILDCOMBAT_REMAKE_MAKEUP,
  TID_GAME_GUILDCOMBAT_SELECTION_MAX,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { ListBox } from '../../controls/ListBox';
import { RectLayer } from '../../controls/RectLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  MIN_LINEUP_LEVEL,
  type SelectionRequest,
  type SiegeHost,
  WAR_STATE_NOTENTER_COUNT,
} from './ports';
import { BAND, DEFENDER_MARK, memberRow, SELECTION_PITCH } from './rows';

export interface SelectionDeps {
  readonly host: SiegeHost;
}

const ID_BANDS = 9000;

export class WndGuildCombatSelection extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_SELECTION;
  readonly #request: SelectionRequest;
  readonly #host: SiegeHost;
  /** `m_vecGuildList`: the character ids of the left list rows. */
  #left: number[] = [];
  /** `m_vecSelectPlayer`: the character ids of the line-up rows. */
  #selected: number[] = [];
  /** `m_uidDefender`, 0 = none. */
  #defender = 0;
  #list1: ListBox | null = null;
  #list2: ListBox | null = null;
  #bands: RectLayer | null = null;

  constructor(request: SelectionRequest, deps: SelectionDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get guildList(): ListBox | null {
    return this.#list1;
  }
  get lineupList(): ListBox | null {
    return this.#list2;
  }
  /** The character ids of the line-up in fight order. */
  get lineup(): readonly number[] {
    return this.#selected;
  }
  /** The character ids of the left list. */
  get guildIds(): readonly number[] {
    return this.#left;
  }
  get defender(): number {
    return this.#defender;
  }
  get defenderMark(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON8);
  }
  get bandLayer(): RectLayer | null {
    return this.#bands;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    // `OnInitialUpdate`: the line-up phase is over
    if (r.warState() !== WAR_STATE_NOTENTER_COUNT) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP));
      this.destroy();
      return;
    }
    this.#list1 = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#list2 = this.getDlgItem<ListBox>(WIDC_LISTBOX2);
    const mark = this.defenderMark;
    mark?.enableWindow(false);
    this.moveParentCenter();
    // `SetMemberSize`
    this.getDlgItem<Static>(WIDC_STATIC3)?.setTitle(
      this.#host.fmt(TID_GAME_GUILDCOMBAT_OFFER_INFO, r.maxWar, r.maxJoin),
    );
    this.updateGuildListBox();
    // the stored line-up
    for (const id of r.selected) this.addCombatPlayer(id);
    this.#defender = r.defender;
    // the two bands of `OnDraw`
    const rect = this.rectClient;
    this.#bands = this.addControl(
      new RectLayer(),
      ID_BANDS,
      { l: 0, t: 0, r: rectWidth(rect), b: rectHeight(rect) },
      WBS_NOFRAME,
    );
    this.#sync();
  }

  // ---- the two lists ---------------------------------------------------------------------------------------------------

  /** `UpDateGuildListBox`: the online members, ascending by level (equal levels in member order). */
  updateGuildListBox(): void {
    const list = this.#list1;
    if (!list) return;
    list.resetContent();
    this.#left = [];
    for (const m of this.#request.members()) {
      list.addString(memberRow(m));
      this.#left.push(m.id);
    }
  }

  /** `AddCombatPlayer`: appends a row to the line-up (a character that is no member is ignored, FIX). */
  addCombatPlayer(id: number): boolean {
    const m = this.#request.memberOf(id);
    if (!m || !this.#list2) return false;
    this.#selected.push(id);
    this.#list2.addString(memberRow(m));
    return true;
  }

  /** `AddGuildPlayer`: appends a row to the left list. */
  #addGuildPlayer(id: number): void {
    const m = this.#request.memberOf(id);
    if (!m || !this.#list1) return;
    this.#left.push(id);
    this.#list1.addString(memberRow(m));
  }

  #removeGuildPlayer(index: number): void {
    this.#list1?.deleteString(index);
    this.#left.splice(index, 1);
  }

  /** `RemoveCombatPlayer`: drops a row of the line-up; the defender goes with his row. */
  #removeCombatPlayer(index: number): void {
    this.#list2?.deleteString(index);
    if (this.#selected[index] === this.#defender) this.#defender = 0;
    this.#selected.splice(index, 1);
  }

  /** `Reset`: no defender, the left list rebuilt, the line-up empty. */
  reset(): void {
    this.#defender = 0;
    this.updateGuildListBox();
    this.#list2?.resetContent();
    this.#selected = [];
    this.#sync();
  }

  /** `SetDefender`. */
  setDefender(id: number): void {
    this.#defender = id;
    this.#sync();
  }

  /** The "D" button and the bands (`OnDraw`). */
  #sync(): void {
    const mark = this.defenderMark;
    if (mark) {
      mark.enableWindow(false);
      if (this.#defender <= 0) {
        mark.setVisible(false);
      } else {
        mark.setVisible(true);
        let i = this.#selected.indexOf(this.#defender);
        if (i < 0) i = this.#selected.length;
        mark.move({ x: DEFENDER_MARK.x, y: i * SELECTION_PITCH + DEFENDER_MARK.y0 });
      }
    }
    const war = this.#request.maxWar * SELECTION_PITCH;
    this.#bands?.setEntries([
      {
        key: 'war',
        x: BAND.l,
        y: BAND.t,
        w: BAND.r - BAND.l,
        h: war,
        fill: 'rgba(0, 0, 220, 0.157)',
      },
      {
        key: 'wait',
        x: BAND.l,
        y: BAND.t + war,
        w: BAND.r - BAND.l,
        h: Math.max(0, BAND.b - (BAND.t + war)),
        fill: 'rgba(220, 0, 0, 0.157)',
      },
    ]);
  }

  // ---- the buttons -----------------------------------------------------------------------------------------------------

  /** `WIDC_BUTTON1`: the selected member of the left list joins the line-up. */
  add(): void {
    const list = this.#list1;
    if (!list) return;
    const sel = list.getCurSel();
    if (sel === -1) return;
    const id = this.#left[sel];
    if (id === undefined) return;
    const r = this.#request;
    if (this.#selected.length >= r.maxJoin) {
      this.#host.box(this.#host.fmt(TID_GAME_GUILDCOMBAT_SELECTION_MAX, r.maxJoin));
      return;
    }
    const m = r.memberOf(id);
    if (m === null) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_NOT_GUILD_MEMBER));
      return;
    }
    if (m.level < MIN_LINEUP_LEVEL) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_LIMIT_LEVEL_NOTICE));
      return;
    }
    if (this.#selected.includes(id)) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_ALREADY_ENTRY));
      return;
    }
    this.addCombatPlayer(id);
    this.#removeGuildPlayer(sel);
    this.#sync();
  }

  /** `WIDC_BUTTON2`: the selected row of the line-up goes back to the left list. */
  remove(): void {
    const list = this.#list2;
    if (!list) return;
    const sel = list.getCurSel();
    if (sel === -1) return;
    const id = this.#selected[sel];
    if (id === undefined) return;
    // a member who is not in the left list (he was added from the stored line-up and removed from the list) goes back to it
    if (!this.#left.includes(id)) this.#addGuildPlayer(id);
    this.#removeCombatPlayer(sel);
    this.#sync();
  }

  /** `WIDC_BUTTON3` / `WIDC_BUTTON4`: the selected row swaps places with its neighbour (the selection follows the row, FIX). */
  moveRow(dir: -1 | 1): void {
    const list = this.#list2;
    if (!list) return;
    const sel = list.getCurSel();
    const other = sel + dir;
    if (sel === -1 || other < 0 || other >= list.getCount()) return;
    const a = list.getText(sel);
    const b = list.getText(other);
    list.setString(other, a);
    list.setString(sel, b);
    const ids = this.#selected;
    const t = ids[sel];
    const u = ids[other];
    if (t === undefined || u === undefined) return;
    ids[sel] = u;
    ids[other] = t;
    list.setCurSel(other);
    this.#sync();
  }

  /** `WIDC_BUTTON7`: the selected row becomes the defender. */
  assignDefender(): void {
    const list = this.#list2;
    if (!list) return;
    const sel = list.getCurSel();
    if (sel === -1) return;
    const id = this.#selected[sel];
    if (id === undefined) return;
    if (this.#selected.length > 1 && this.#request.isMaster(id)) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_MASTER_NOT_ASSIGN_DEFENDER));
      return;
    }
    this.setDefender(id);
  }

  /** `WIDC_FINISH`. */
  finish(): void {
    const r = this.#request;
    if (this.#defender <= 0) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_NOT_ASSIGN_DEFENDER));
      return;
    }
    if (r.warState() !== WAR_STATE_NOTENTER_COUNT) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP));
      this.destroy();
      return;
    }
    if (this.#selected.length === 0) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_HAVENOT_PLAYER));
      return;
    }
    if (!this.#selected.some((id) => r.isMasterOrKingpin(id))) {
      this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_HAVENOT_MASTER));
      return;
    }
    r.submit(this.#defender, [...this.#selected]);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_BUTTON1:
          this.add();
          return true;
        case WIDC_BUTTON2:
          this.remove();
          return true;
        case WIDC_BUTTON3:
          this.moveRow(-1);
          return true;
        case WIDC_BUTTON4:
          this.moveRow(1);
          return true;
        case WIDC_BUTTON7:
          this.assignDefender();
          return true;
        case WIDC_RESET:
          this.#host.confirm(
            this.#host.text(TID_GAME_GUILDCOMBAT_REMAKE_MAKEUP),
            () => {
              this.reset();
            },
            this,
          );
          return true;
        case WIDC_FINISH:
          this.finish();
          return true;
        case WIDC_CLOSE:
          this.destroy();
          return true;
        default:
          break;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
