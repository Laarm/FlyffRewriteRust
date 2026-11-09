// `WndGuildCombatRankPerson` (`CWndGuildCombatRank_Person`, `src:_Interface/WndField.cpp:18270-18480`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-50): `APP_GUILDCOMBAT_RANK_P` (575, 400 x 384), the
// cross-siege ranking of the players (`GC_PLAYERPOINT`, the NPC's "ranking" entry). A tab control (`WIDC_TABCTRL1`) with five pages of `WndGuildCombatRankClass` - all, Mercenary,
// Magician, Acrobat, Assist - and a close button. The pages are filled from the view of the request (a player lands on the page of his job line - base, pro, master and hero - and on the
// first page; jobs of no line, the vagrant, appear nowhere); the view changes when a name arrives, and every page follows. The original's `SetButtonLength( 100 )` of the tab clicks is not
// ported: a tab answers a click on its drawn width.

import {
  APP_GUILDCOMBAT_RANK_P,
  APP_GUILDCOMBAT_RANKINGCLASS,
  WIDC_CLOSE,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import type { TabCtrl } from '../../controls/TabCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { RankPersonRequest, RankView } from './ports';
import { WndGuildCombatRankClass } from './WndGuildCombatRankClass';

export interface RankPersonDeps {
  readonly getText: (tid: number) => string;
}

export class WndGuildCombatRankPerson extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_RANK_P;
  readonly #request: RankPersonRequest;
  readonly #deps: RankPersonDeps;
  #tab: TabCtrl | null = null;
  #pages: WndGuildCombatRankClass[] = [];
  #applied: RankView | null = null;
  #off: (() => void) | null = null;

  constructor(request: RankPersonRequest, deps: RankPersonDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  /** The five pages: all, Mercenary, Magician, Acrobat, Assist. */
  get pages(): readonly WndGuildCombatRankClass[] {
    return this.#pages;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      this.#tab = tab;
      this.#request.tabs.forEach((text, i) => {
        const page = new WndGuildCombatRankClass({ getText: this.#deps.getText });
        page.attach(tab, APP_GUILDCOMBAT_RANKINGCLASS);
        tab.insertItem(i, { text, page });
        this.#pages.push(page);
      });
    }
    this.#apply(this.#request.view.get());
    this.#off = this.#request.view.subscribe(() => {
      this.#apply(this.#request.view.get());
    });
    this.moveParentCenter();
  }

  #apply(v: RankView): void {
    this.#applied = v;
    this.#pages.forEach((page, i) => {
      page.apply(v.pages[i] ?? [], v.own[i] ?? null);
    });
  }

  override process(now: number): void {
    const v = this.#request.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_CLOSE) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
