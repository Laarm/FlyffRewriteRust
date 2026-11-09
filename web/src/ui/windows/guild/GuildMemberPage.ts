// `GuildMemberPage` (`CWndGuildTabMember`, `src:_Interface/WndGuildTabMember.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-45): the member tab of `APP_GUILD`
// (`APP_GUILD_TABMEMBER_EX`, 404 x 300). The layout has only the five sort buttons (`WIDC_MLEVEL`, `WIDC_JOB`, `WIDC_LEVEL`, `WIDC_NAME`, `WIDC_ALIAS`); the rows are ONE
// `GuildMemberList` over the area below them (18 px rows, 14 visible). The five buttons sort locally with the comparators and the flag machine of the original
// (`memberSort.ts`) and call NO port; the right-click menu is the feature's (`actions.memberMenu`, `actions.pickMember`); the tooltip of a row is
// "name / contribution points / contribution penya / surrenders".

import {
  WIDC_ALIAS,
  WIDC_JOB,
  WIDC_LEVEL,
  WIDC_MLEVEL,
  WIDC_NAME,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_TOOLTIP_EXPMERIT,
  TID_GAME_TOOLTIP_GIVEUPNUM,
  TID_GAME_TOOLTIP_PENYAMERIT,
} from '../../../gen/defs/text';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { CodePage } from '../char/CodePage';
import type { MoverMenu } from '../npc/MoverMenu';
import { GuildMemberList } from './GuildMemberList';
import { LIST_TOP } from './memberIcons';
import { MemberListSort, type MemberSortKey } from './memberSort';
import type { GuildActions, GuildMemberRow, GuildView } from './ports';

const ID_LIST = 0x7f11;
/** The right edge of the list control: the docked bar sits at 376..391 of the page (the original put it 25 px from the right edge of the tab page). */
const LIST_RIGHT = 391;
/** The bottom edge of the list control (the original left 10 px below). */
const LIST_BOTTOM = 296;

/** The sort button each layout id stands for. */
const BUTTONS: Readonly<Record<number, MemberSortKey>> = {
  [WIDC_MLEVEL]: 'mlevel',
  [WIDC_JOB]: 'job',
  [WIDC_LEVEL]: 'level',
  [WIDC_NAME]: 'name',
  [WIDC_ALIAS]: 'alias',
};

export interface MemberPageDeps {
  readonly actions: GuildActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  /** The popup of the right click (`m_wndMenu.CreateMenu( this )`). */
  readonly menu: MoverMenu | null;
}

export class GuildMemberPage extends CodePage {
  readonly #deps: MemberPageDeps;
  readonly #sort = new MemberListSort();
  #list: GuildMemberList | null = null;
  /** The rows as the view gave them (character id order). */
  #raw: readonly GuildMemberRow[] = [];
  /** The rows in display order. */
  #shown: readonly GuildMemberRow[] = [];

  constructor(deps: MemberPageDeps) {
    super();
    this.#deps = deps;
  }

  get list(): GuildMemberList | null {
    return this.#list;
  }
  get sort(): MemberListSort {
    return this.#sort;
  }
  /** The rows in display order (what the list shows). */
  get rows(): readonly GuildMemberRow[] {
    return this.#shown;
  }
  get selectedId(): number | null {
    return this.#list?.selectedId ?? null;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const size = this.getClientRect(false);
    this.#list = this.addControl(
      new GuildMemberList({
        context: (id, at) => {
          this.#openMenu(id, at);
        },
        hover: (row, at) => {
          this.#hover(row, at);
        },
      }),
      ID_LIST,
      rect(0, LIST_TOP, Math.min(LIST_RIGHT, size.r), Math.min(LIST_BOTTOM, size.b)),
    );
    this.#list.setMembers(this.#shown);
  }

  /** `UpdateData`: the rows of the view, sorted in the direction of the last click (the member level before any click). */
  apply(v: GuildView): void {
    this.#raw = v.members;
    this.#shown = this.#sort.sortRebuild(this.#raw);
    this.#list?.setMembers(this.#shown);
  }

  /** A click on one of the five sort buttons: reorders the view, calls no port. */
  sortBy(key: MemberSortKey): void {
    this.#shown = this.#sort.sortClick(this.#raw, key);
    this.#list?.setMembers(this.#shown);
  }

  #openMenu(id: number, at: Point): void {
    const menu = this.#deps.menu;
    if (!menu) return;
    const entries = this.#deps.actions.memberMenu(id);
    if (entries.length === 0) return;
    menu.onPick = (entry) => {
      this.#deps.actions.pickMember(id, entry);
    };
    menu.show(
      entries.map((e) => ({ id: e.id, text: e.text })),
      at,
    );
  }

  /** The tooltip of a row: the name and the contribution counters. */
  tooltipText(row: GuildMemberRow): string {
    const t = this.#deps.getText;
    return [
      row.name,
      `${t(TID_GAME_TOOLTIP_EXPMERIT)} ${String(row.givePxp)}`,
      `${t(TID_GAME_TOOLTIP_PENYAMERIT)} ${String(row.giveGold)}`,
      `${t(TID_GAME_TOOLTIP_GIVEUPNUM)} ${String(row.surrender)}`,
    ].join('\n');
  }

  #hover(row: GuildMemberRow | null, at: Rect | null): void {
    if (!row || !at) {
      this.#deps.tooltip.cancel();
      return;
    }
    this.#deps.tooltip.put(this, this.tooltipText(row), at, 3);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const key = BUTTONS[id];
      if (key !== undefined) {
        this.sortBy(key);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
