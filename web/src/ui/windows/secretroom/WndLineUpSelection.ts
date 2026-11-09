// `WndLineUpSelection` (`CWndSecretRoomSelection`, `src:_Interface/WndSecretRoom.cpp:35-400, 824-850`, and its original `CWndGuildCombat1to1Selection`, `WndGuildCombat1to1.cpp:20-545, 730-760`;
// ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3, §28.5; WP-UI-56, WP-UI-57): the line-up editor, ONE class for `APP_SECRETROOM_SELECTION` (702, `kind: 'secretRoom'`) and `APP_GUILDCOMBAT_1TO1_SELECTION`
// (672, `kind: 'gc1to1'`): the secret-room window is a copy of the 1:1 one. `LISTBOX1` lists the online guild members (ascending by level), `LISTBOX2` the line-up in order ("No.n  Lv..",
// renumbered after a removal). The two kinds differ only where named here:
//
//   * at open the stored ids fill `LISTBOX2` in order (an id that is no member of the guild is skipped: FIX, the original dereferenced `end()`); 702 also removes them from `LISTBOX1`, 672 does
//     NOT (KEEP: a pre-selected player stays in both lists, adding him again says `ALREADY_ENTRY`, removing him does not duplicate him on the left);
//   * ADD (`BUTTON1`; nothing selected: nothing): the size at the maximum -> `SELECTION_MAX( max )` (FIX: the original allowed max + 1), the level under the limit -> `LIMIT_LEVEL` (secret room:
//     30, FIX of the original's `m_nMinGuildMemberNum`), not a member -> `NOT_GUILD_MEMBER`, already in -> `ALREADY_ENTRY`; then the player moves right;
//   * REMOVE (`BUTTON2`): the secret room refuses the guild master (`DONTREMOVE_GUILDMASTER`), the 1:1 combat has no such check; a player that is not in the left list goes back to its END
//     (KEEP: `AddGuildPlayer` appends, an offline pre-selected player too);
//   * `BUTTON3` / `BUTTON4` (672 only: the 702 layout has none) move a row up / down, the selection follows;
//   * RESET asks `REMAKE_MAKEUP`; OK rebuilds the left list and empties the right one;
//   * FINISH: 672 only: `canEdit()` false -> `CANNOT_MAKEUP` and the window closes; size < min -> `MIN_PLAYER( min )`; size 0 -> `HAVENOT_PLAYER`; no master (702) / no master or kingpin (672) ->
//     `HAVENOT_MASTER`; else `submit( ids )` and the window closes. The 672 window also refuses to open while `canEdit()` is false.
//
// Texts: the `TID_GAME_SECRETROOM_*` family for 702, the `TID_GAME_GUILDCOMBAT1TO1_*` family for 672.

import {
  APP_GUILDCOMBAT_1TO1_SELECTION,
  APP_SECRETROOM_SELECTION,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_BUTTON4,
  WIDC_CLOSE,
  WIDC_FINISH,
  WIDC_LISTBOX1,
  WIDC_LISTBOX2,
  WIDC_RESET,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT1TO1_ALREADY_ENTRY,
  TID_GAME_GUILDCOMBAT1TO1_CANNOT_MAKEUP,
  TID_GAME_GUILDCOMBAT1TO1_HAVENOT_MASTER,
  TID_GAME_GUILDCOMBAT1TO1_HAVENOT_PLAYER,
  TID_GAME_GUILDCOMBAT1TO1_LIMIT_LEVEL_NOTICE,
  TID_GAME_GUILDCOMBAT1TO1_NOT_GUILD_MEMBER,
  TID_GAME_GUILDCOMBAT1TO1_REMAKE_MAKEUP,
  TID_GAME_GUILDCOMBAT1TO1_SELECTION_MAX,
  TID_GAME_GUILDCOMBAT_1TO1_MIN_PLAYER,
  TID_GAME_SECRETROOM_ALREADY_ENTRY,
  TID_GAME_SECRETROOM_DONTREMOVE_GUILDMASTER,
  TID_GAME_SECRETROOM_HAVENOT_MASTER,
  TID_GAME_SECRETROOM_HAVENOT_PLAYER,
  TID_GAME_SECRETROOM_LIMIT_LEVEL,
  TID_GAME_SECRETROOM_MIN_PLAYER,
  TID_GAME_SECRETROOM_NOT_GUILD_MEMBER,
  TID_GAME_SECRETROOM_REMAKE_MAKEUP,
  TID_GAME_SECRETROOM_SELECTION_MAX,
} from '../../../gen/defs/text';
import type { ListBox } from '../../controls/ListBox';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LineUpKind, LineUpRequest, SecretRoomHost } from './ports';
import { lineupRow, memberRow } from './rows';

export interface LineUpDeps {
  readonly host: SecretRoomHost;
}

/** The texts of one kind. */
interface Texts {
  readonly max: number;
  readonly level: number;
  readonly notMember: number;
  readonly already: number;
  readonly master: number;
  readonly noPlayer: number;
  readonly minPlayer: number;
  readonly remake: number;
  readonly cannot: number;
}

const TEXTS: Readonly<Record<LineUpKind, Texts>> = {
  secretRoom: {
    max: TID_GAME_SECRETROOM_SELECTION_MAX,
    level: TID_GAME_SECRETROOM_LIMIT_LEVEL,
    notMember: TID_GAME_SECRETROOM_NOT_GUILD_MEMBER,
    already: TID_GAME_SECRETROOM_ALREADY_ENTRY,
    master: TID_GAME_SECRETROOM_HAVENOT_MASTER,
    noPlayer: TID_GAME_SECRETROOM_HAVENOT_PLAYER,
    minPlayer: TID_GAME_SECRETROOM_MIN_PLAYER,
    remake: TID_GAME_SECRETROOM_REMAKE_MAKEUP,
    // the secret room has no such check
    cannot: 0,
  },
  gc1to1: {
    max: TID_GAME_GUILDCOMBAT1TO1_SELECTION_MAX,
    level: TID_GAME_GUILDCOMBAT1TO1_LIMIT_LEVEL_NOTICE,
    notMember: TID_GAME_GUILDCOMBAT1TO1_NOT_GUILD_MEMBER,
    already: TID_GAME_GUILDCOMBAT1TO1_ALREADY_ENTRY,
    master: TID_GAME_GUILDCOMBAT1TO1_HAVENOT_MASTER,
    noPlayer: TID_GAME_GUILDCOMBAT1TO1_HAVENOT_PLAYER,
    minPlayer: TID_GAME_GUILDCOMBAT_1TO1_MIN_PLAYER,
    remake: TID_GAME_GUILDCOMBAT1TO1_REMAKE_MAKEUP,
    cannot: TID_GAME_GUILDCOMBAT1TO1_CANNOT_MAKEUP,
  },
};

export class WndLineUpSelection extends WndNeuz {
  // typed `number`: 702 or 672 by the kind
  readonly appId: number;
  readonly #request: LineUpRequest;
  readonly #host: SecretRoomHost;
  readonly #t: Texts;
  /** The character ids of the left list rows. */
  #left: number[] = [];
  /** The character ids of the line-up rows. */
  #selected: number[] = [];
  #list1: ListBox | null = null;
  #list2: ListBox | null = null;

  constructor(request: LineUpRequest, deps: LineUpDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.#t = TEXTS[request.kind];
    this.appId =
      request.kind === 'secretRoom' ? APP_SECRETROOM_SELECTION : APP_GUILDCOMBAT_1TO1_SELECTION;
    this.putRegInfo = false;
  }

  get kind(): LineUpKind {
    return this.#request.kind;
  }
  get guildList(): ListBox | null {
    return this.#list1;
  }
  get lineupList(): ListBox | null {
    return this.#list2;
  }
  /** The character ids of the line-up in order. */
  get lineup(): readonly number[] {
    return this.#selected;
  }
  /** The character ids of the left list. */
  get guildIds(): readonly number[] {
    return this.#left;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    // `CWndGuildCombat1to1Selection::OnInitialUpdate`: the line-up may be edited only in the open state
    if (r.kind === 'gc1to1' && !r.canEdit()) {
      this.#host.box(this.#host.text(this.#t.cannot));
      this.destroy();
      return;
    }
    this.#list1 = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#list2 = this.getDlgItem<ListBox>(WIDC_LISTBOX2);
    this.moveParentCenter();
    this.updateGuildListBox();
    // the stored line-up
    for (const id of r.selected) this.addCombatPlayer(id);
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

  /** `AddCombatPlayer`: appends a row to the line-up (a character that is no member is ignored, FIX); the secret-room editor takes him out of the left list, the 1:1 editor does not. */
  addCombatPlayer(id: number): boolean {
    const m = this.#request.memberOf(id);
    if (!m || !this.#list2) return false;
    this.#selected.push(id);
    this.#list2.addString(lineupRow(this.#selected.length, m));
    if (this.#request.kind === 'secretRoom') {
      const at = this.#left.indexOf(id);
      if (at >= 0) this.#removeGuildPlayer(at);
    }
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

  /** The right list again from the ids (the numbers follow the order). */
  #rebuildRight(): void {
    const list = this.#list2;
    if (!list) return;
    list.resetContent();
    this.#selected.forEach((id, i) => {
      const m = this.#request.memberOf(id);
      if (m) list.addString(lineupRow(i + 1, m));
    });
  }

  /** `Reset`: the left list rebuilt, the line-up empty. */
  reset(): void {
    this.updateGuildListBox();
    this.#list2?.resetContent();
    this.#selected = [];
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
    if (this.#selected.length >= r.max) {
      this.#host.box(this.#host.fmt(this.#t.max, r.max));
      return;
    }
    const m = r.memberOf(id);
    if (m !== null && m.level < r.minLevel) {
      this.#host.box(this.#host.text(this.#t.level));
      return;
    }
    if (m === null) {
      this.#host.box(this.#host.text(this.#t.notMember));
      return;
    }
    if (this.#selected.includes(id)) {
      this.#host.box(this.#host.text(this.#t.already));
      return;
    }
    // the secret-room editor takes the member out of the left list inside `addCombatPlayer`; the 1:1 editor's button removes the selected left row itself
    if (this.addCombatPlayer(id) && r.kind === 'gc1to1') this.#removeGuildPlayer(sel);
  }

  /** `WIDC_BUTTON2`: the selected row of the line-up goes back to the left list. */
  remove(): void {
    const list = this.#list2;
    if (!list) return;
    const sel = list.getCurSel();
    if (sel === -1) return;
    const id = this.#selected[sel];
    if (id === undefined) return;
    if (this.#request.kind === 'secretRoom' && this.#request.isMaster(id)) {
      this.#host.box(this.#host.text(TID_GAME_SECRETROOM_DONTREMOVE_GUILDMASTER));
      return;
    }
    // a player who is not in the left list goes back to its end (also an offline pre-selected player)
    if (!this.#left.includes(id)) this.#addGuildPlayer(id);
    this.#selected.splice(sel, 1);
    this.#rebuildRight();
  }

  /** `WIDC_BUTTON3` / `WIDC_BUTTON4` (672): the selected row swaps places with its neighbour, the selection follows the row. */
  moveRow(dir: -1 | 1): void {
    const list = this.#list2;
    if (!list) return;
    const sel = list.getCurSel();
    const other = sel + dir;
    if (sel === -1 || other < 0 || other >= this.#selected.length) return;
    const t = this.#selected[sel];
    const u = this.#selected[other];
    if (t === undefined || u === undefined) return;
    this.#selected[sel] = u;
    this.#selected[other] = t;
    this.#rebuildRight();
    list.setCurSel(other);
  }

  /** `WIDC_FINISH`. */
  finish(): void {
    const r = this.#request;
    if (r.kind === 'gc1to1' && !r.canEdit()) {
      this.#host.box(this.#host.text(this.#t.cannot));
      this.destroy();
      return;
    }
    if (this.#selected.length < r.min) {
      this.#host.box(this.#host.fmt(this.#t.minPlayer, r.min));
      return;
    }
    if (this.#selected.length === 0) {
      this.#host.box(this.#host.text(this.#t.noPlayer));
      return;
    }
    const lead = (id: number): boolean =>
      r.isMaster(id) || (r.kind === 'gc1to1' && r.isKingpin(id));
    if (!this.#selected.some(lead)) {
      this.#host.box(this.#host.text(this.#t.master));
      return;
    }
    r.submit([...this.#selected]);
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
        case WIDC_RESET:
          this.#host.confirm(
            this.#host.text(this.#t.remake),
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
