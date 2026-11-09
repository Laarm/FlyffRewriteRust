// `WndParty` (`CWndParty`, `src:_Interface/WndParty.cpp:40-420`, ref/22 §17 H, Plan/42 §19.4 H, 45 §22.2; WP-UI-39): `APP_PARTY` (448 x 336),
// the party window. The top shows the party (name, level, EXP %, points) and the share modes (radios: EXP by level / contribution, items
// individually / sequentially / leader / randomly, and "Allow Joining" for the party finder); the tab control below holds the member list
// (`PartyInfoPage`) and the party skills (`PartySkillPage`); the buttons leave or kick, hand over the lead, change to an advanced party and
// open the simple display (`APP_PARTY_QUICK`).
//
// The state of every control comes from the view (`OnDraw` of the original rewrote it every frame, so a radio a non-leader clicked springs
// back at once); the decisions behind a click (the leader checks, the lines `TID_DIAG_0008` ...) are the actions of 45.

import {
  APP_PARTY,
  APP_PARTY_INFO,
  APP_PARTY_SKILL,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_CHANGE,
  WIDC_EXP,
  WIDC_EXP_SHARE,
  WIDC_ITEM_SHARE,
  WIDC_LEAVE,
  WIDC_LEVEL,
  WIDC_NAME,
  WIDC_POINT,
  WIDC_RADIO2,
  WIDC_RADIO3,
  WIDC_RADIO4,
  WIDC_RADIO6,
  WIDC_RADIO7,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import { TID_APP_INFOMATION, TID_APP_SKILL } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { TabCtrl } from '../../controls/TabCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PartyInfoPage } from './PartyInfoPage';
import { PartySkillPage } from './PartySkillPage';
import { EMPTY_PARTY_VIEW, type PartyActions, type PartyView, type ViewSource } from './ports';

export interface PartyDeps {
  readonly view: ViewSource<PartyView>;
  readonly actions: PartyActions;
  readonly getText: (tid: number) => string;
}

/** The radios of the item share, in mode order (`m_nTroupeShareItem` 0..3). */
const ITEM_RADIOS = [WIDC_ITEM_SHARE, WIDC_RADIO2, WIDC_RADIO3, WIDC_RADIO4] as const;
/** The radios of the EXP share, in mode order (`m_nTroupsShareExp` 0..1). */
const EXP_RADIOS = [WIDC_EXP_SHARE, WIDC_RADIO6] as const;

export class WndParty extends WndNeuz {
  readonly appId = APP_PARTY;
  readonly #deps: PartyDeps;
  #tab: TabCtrl | null = null;
  #info: PartyInfoPage | null = null;
  #skills: PartySkillPage | null = null;
  #applied: PartyView = EMPTY_PARTY_VIEW;
  #unsubscribe: (() => void) | null = null;
  /** The tab to select when the layout is built (`SerializeRegInfo`). */
  #restoreTab = 0;

  constructor(deps: PartyDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get infoPage(): PartyInfoPage | null {
    return this.#info;
  }
  get skillPage(): PartySkillPage | null {
    return this.#skills;
  }
  get leaveButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_LEAVE);
  }
  get changeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CHANGE);
  }
  /** `WIDC_BUTTON1`: hand over the lead. */
  get transferButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  /** `WIDC_BUTTON2`: the simple party display. */
  get quickButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  /** `WIDC_RADIO7`: "Allow Joining". */
  get allowJoinRadio(): Button | null {
    return this.getDlgItem<Button>(WIDC_RADIO7);
  }
  itemRadio(mode: number): Button | null {
    const id = ITEM_RADIOS[mode];
    return id === undefined ? null : this.getDlgItem<Button>(id);
  }
  expRadio(mode: number): Button | null {
    const id = EXP_RADIOS[mode];
    return id === undefined ? null : this.getDlgItem<Button>(id);
  }
  stat(id: number): Static | null {
    return this.getDlgItem<Static>(id);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      this.#tab = tab;
      const info = new PartyInfoPage(this.#deps);
      info.attach(tab, APP_PARTY_INFO);
      const skills = new PartySkillPage(this.#deps);
      skills.attach(tab, APP_PARTY_SKILL);
      this.#info = info;
      this.#skills = skills;
      tab.insertItem(0, { text: this.#deps.getText(TID_APP_INFOMATION), page: info });
      tab.insertItem(1, { text: this.#deps.getText(TID_APP_SKILL), page: skills });
      tab.setCurSel(this.#restoreTab);
    }
    this.#apply(this.#deps.view.get());
    this.#unsubscribe = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.moveParentCenter();
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  /** `OnDraw`: the buttons, the statics and the radios follow the party. */
  #apply(v: PartyView): void {
    this.#applied = v;
    const enough = v.members.length >= 2;
    this.leaveButton?.enableWindow(enough);
    this.transferButton?.enableWindow(enough);
    this.quickButton?.enableWindow(enough);
    this.changeButton?.enableWindow(v.canChange);
    this.stat(WIDC_NAME)?.setTitle(v.nameText);
    this.stat(WIDC_LEVEL)?.setTitle(String(v.level));
    this.stat(WIDC_EXP)?.setTitle(v.expText);
    this.stat(WIDC_POINT)?.setTitle(String(v.point));
    this.#syncRadios(v);
  }

  #syncRadios(v: PartyView): void {
    EXP_RADIOS.forEach((id, mode) => {
      this.getDlgItem<Button>(id)?.setCheck(v.shareExp === mode);
    });
    ITEM_RADIOS.forEach((id, mode) => {
      this.getDlgItem<Button>(id)?.setCheck(v.shareItem === mode);
    });
    const join = this.allowJoinRadio;
    if (join) {
      join.setVisible(v.findEnabled);
      join.setCheck(v.allowJoin);
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg !== Wnm.CLICKED) return super.onChildNotify(msg, id, src);
    const a = this.#deps.actions;
    const sel = this.#info?.selectedId ?? null;
    switch (id) {
      case WIDC_LEAVE:
        a.leave(sel);
        this.#info?.clearSelection();
        return true;
      case WIDC_BUTTON1:
        a.transferLeader(sel);
        return true;
      case WIDC_CHANGE:
        a.openChange();
        return true;
      case WIDC_BUTTON2:
        a.toggleQuick();
        return true;
      case WIDC_RADIO7:
        if (this.#applied.leader) {
          const allow = !this.#applied.allowJoin;
          this.allowJoinRadio?.setCheck(allow);
          a.setAllowJoin(allow);
        }
        return true;
      default:
        break;
    }
    const itemMode = ITEM_RADIOS.indexOf(id as (typeof ITEM_RADIOS)[number]);
    if (itemMode >= 0) {
      a.setItemShare(itemMode);
      this.#syncRadios(this.#applied);
      return true;
    }
    const expMode = EXP_RADIOS.indexOf(id as (typeof EXP_RADIOS)[number]);
    if (expMode >= 0) {
      a.setExpShare(expMode);
      this.#syncRadios(this.#applied);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  // ---- `SerializeRegInfo`: the selected tab -------------------------------------------------------------------

  override serializeRegInfo(): unknown {
    return { tab: this.#tab?.getCurSel() ?? this.#restoreTab };
  }

  override restoreRegInfo(v: unknown): void {
    const tab = (v as { tab?: unknown } | null)?.tab;
    if (typeof tab !== 'number' || tab < 0 || tab > 1) return;
    this.#restoreTab = tab;
    this.#tab?.setCurSel(tab);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    super.onDestroy();
  }
}
