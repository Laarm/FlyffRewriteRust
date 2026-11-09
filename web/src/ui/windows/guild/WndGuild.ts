// `WndGuild` (`CWndGuild`, `src:_Interface/WndGuild.cpp:20-170`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-45): `APP_GUILD` (448 x 368, hotkey G), the tab host of the guild.
// One tab control (`WIDC_TABCTRL1`) with five pages in the order of the original: information (`GuildInfoPage`), members (`GuildMemberPage`), appellation (`GuildAppPage`),
// war (`GuildWarPage`) and - as in the original's `__GUILD_HOUSE` build - the agit page (`GuildPowerPage`). The window opens blank for a player without a guild and
// rebuilds every page on any change of the view (`UpdateDataAll`); it centres itself. The shell draws views and sends actions (`GuildPorts`); the decisions are the feature's.

import {
  APP_GUILD,
  APP_GUILD_TABAPPELLATION,
  APP_GUILD_TABGUILDWAR,
  APP_GUILD_TABINFO,
  APP_GUILD_TABMEMBER_EX,
  APP_GUILD_TAPPOWER,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_TOOLTIP_AGIT,
  TID_GAME_TOOLTIP_APPELLATION,
  TID_GAME_TOOLTIP_GUILDINFO,
  TID_GAME_TOOLTIP_GUILDMEMBER,
  TID_GAME_TOOLTIP_GUILDWAR,
} from '../../../gen/defs/text';
import type { TabCtrl } from '../../controls/TabCtrl';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import { MoverMenu } from '../npc/MoverMenu';
import { GuildAppPage } from './GuildAppPage';
import { GuildInfoPage } from './GuildInfoPage';
import { GuildMemberPage } from './GuildMemberPage';
import { GuildPowerPage } from './GuildPowerPage';
import { GuildWarPage } from './GuildWarPage';
import type { GuildPorts, GuildView, PowerView } from './ports';

export interface GuildDeps {
  readonly ports: GuildPorts;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

/** The tab index of each page. */
export const GUILD_TAB = { info: 0, member: 1, appellation: 2, war: 3, agit: 4 } as const;

export class WndGuild extends WndNeuz {
  readonly appId = APP_GUILD;
  readonly #deps: GuildDeps;
  #tab: TabCtrl | null = null;
  #info: GuildInfoPage | null = null;
  #member: GuildMemberPage | null = null;
  #app: GuildAppPage | null = null;
  #war: GuildWarPage | null = null;
  #power: GuildPowerPage | null = null;
  #menu: MoverMenu | null = null;
  #appliedView: GuildView | null = null;
  #appliedPower: PowerView | null = null;
  #off: (() => void)[] = [];
  /** The tab to select when the layout is built (`SerializeRegInfo`). */
  #restoreTab = 0;

  constructor(deps: GuildDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get infoPage(): GuildInfoPage | null {
    return this.#info;
  }
  get memberPage(): GuildMemberPage | null {
    return this.#member;
  }
  get appPage(): GuildAppPage | null {
    return this.#app;
  }
  get warPage(): GuildWarPage | null {
    return this.#war;
  }
  get powerPage(): GuildPowerPage | null {
    return this.#power;
  }
  get memberMenu(): MoverMenu | null {
    return this.#menu;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const d = this.#deps;
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      this.#tab = tab;
      const menu = new MoverMenu();
      menu.attach(this);
      this.#menu = menu;
      const info = new GuildInfoPage(d.ports.actions);
      info.attach(tab, APP_GUILD_TABINFO);
      const app = new GuildAppPage({ actions: d.ports.actions, atlases: d.atlases });
      app.attach(tab, APP_GUILD_TABAPPELLATION);
      const member = new GuildMemberPage({
        actions: d.ports.actions,
        tooltip: d.tooltip,
        getText: d.getText,
        menu,
      });
      member.attach(tab, APP_GUILD_TABMEMBER_EX);
      const war = new GuildWarPage({ actions: d.ports.actions, now: () => d.ports.now() });
      war.attach(tab, APP_GUILD_TABGUILDWAR);
      const power = new GuildPowerPage(d.ports.actions);
      power.attach(tab, APP_GUILD_TAPPOWER);
      this.#info = info;
      this.#app = app;
      this.#member = member;
      this.#war = war;
      this.#power = power;
      tab.insertItem(GUILD_TAB.info, { text: d.getText(TID_GAME_TOOLTIP_GUILDINFO), page: info });
      tab.insertItem(GUILD_TAB.member, {
        text: d.getText(TID_GAME_TOOLTIP_GUILDMEMBER),
        page: member,
      });
      tab.insertItem(GUILD_TAB.appellation, {
        text: d.getText(TID_GAME_TOOLTIP_APPELLATION),
        page: app,
      });
      tab.insertItem(GUILD_TAB.war, { text: d.getText(TID_GAME_TOOLTIP_GUILDWAR), page: war });
      tab.insertItem(GUILD_TAB.agit, { text: d.getText(TID_GAME_TOOLTIP_AGIT), page: power });
      tab.setCurSel(this.#restoreTab);
    }
    this.#apply(d.ports.view.get());
    this.#applyPower(d.ports.power.get());
    this.#off = [
      d.ports.view.subscribe(() => {
        this.#apply(d.ports.view.get());
      }),
      d.ports.power.subscribe(() => {
        this.#applyPower(d.ports.power.get());
      }),
    ];
    this.moveParentCenter();
  }

  /** `UpdateDataAll`. */
  #apply(v: GuildView): void {
    this.#appliedView = v;
    this.#info?.apply(v);
    this.#member?.apply(v);
    this.#app?.apply(v);
    this.#war?.apply(v);
  }

  #applyPower(p: PowerView): void {
    this.#appliedPower = p;
    this.#app?.applyPower(p);
    this.#power?.applyPower(p);
  }

  override process(now: number): void {
    const v = this.#deps.ports.view.get();
    if (v !== this.#appliedView) this.#apply(v);
    const p = this.#deps.ports.power.get();
    if (p !== this.#appliedPower) this.#applyPower(p);
    super.process(now);
  }

  /** `SetCurTab( index )`. */
  setCurTab(index: number): void {
    this.#tab?.setCurSel(index);
  }

  // ---- `SerializeRegInfo`: the selected tab -------------------------------------------------------------------

  override serializeRegInfo(): unknown {
    return { tab: this.#tab?.getCurSel() ?? this.#restoreTab };
  }

  override restoreRegInfo(v: unknown): void {
    const tab = (v as { tab?: unknown } | null)?.tab;
    if (typeof tab !== 'number' || tab < 0 || tab > GUILD_TAB.agit) return;
    this.#restoreTab = tab;
    this.#tab?.setCurSel(tab);
  }

  override onDestroy(): void {
    for (const off of this.#off) off();
    this.#off = [];
    super.onDestroy();
  }
}
