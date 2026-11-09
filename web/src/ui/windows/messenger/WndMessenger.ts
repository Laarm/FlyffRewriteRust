// `WndMessenger` (`CWndMessengerEx`, `src:_Interface/WndMessenger.cpp:396-950`, Plan/42 §19.4 I, 45 §24.1; WP-UI-43): `APP_MESSENGER_EX` (352 x 368, hotkey E, bottom
// right of the screen), the messenger. The header shows the own state (the big icon at 8,8, the name twice for the fake bold, `(status)` below), the server name
// (campus points on the campus tab), the mail picture while a mail waits and the flashing notes button. The state arrow opens the menu of the seven states; the five
// sort buttons sort the page of the selected tab; the tab control holds the friend page, the guild page and the campus page.
//
// The shell draws a view and sends actions (`MessengerPorts`); the decisions (the lines of a double click, the entries of the friend menu, the sends) are the feature's.

import {
  APP_MESSENGER_EX,
  APP_MESSENGER_TAB_CAMPUS,
  WIDC_ADD,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_CHANNEL,
  WIDC_CUSTOM1,
  WIDC_JOB,
  WIDC_LEVEL,
  WIDC_NAME,
  WIDC_STATE,
  WIDC_TABCTRL1,
} from '../../../gen/defs/resdata';
import {
  TID_APP_COMMUNITY_CAMPUS,
  TID_APP_COMMUNITY_FRIEND,
  TID_APP_COMPANY,
  TID_FRS_ABSENT,
  TID_FRS_EAT,
  TID_FRS_HARDPLAY,
  TID_FRS_MOVE,
  TID_FRS_OFFLINE_STATUS,
  TID_FRS_ONLINE_STATUS,
  TID_FRS_REST,
  TID_GAME_CAMPUS_POINTS,
  TID_MAIL_RECEIVE,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TabCtrl } from '../../controls/TabCtrl';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Rect, rectHeight, rectWidth } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { MoverMenu } from '../npc/MoverMenu';
import {
  bigStateSprite,
  cutServerName,
  FLASH,
  flashStep,
  HEADER,
  HEADER_TEXT_COLOR,
  MAIL_PICTURE,
  MAIL_PICTURE_SIZE,
  PLAYER_DATA_ATLAS,
} from './icons';
import { MessengerListPage } from './MessengerListPage';
import {
  type CampusTabPorts,
  EMPTY_MESSENGER_VIEW,
  FRS,
  type MessengerPorts,
  type MessengerView,
} from './ports';
import type { SortKey } from './sort';
import { WndCampusTab } from './WndCampusTab';

export interface MessengerDeps {
  readonly ports: MessengerPorts;
  /** The campus ports are read when the window is built and again every frame (they may be installed later). */
  readonly campus: () => CampusTabPorts;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

/** The tab order (`InsertItem( 0 .. 2 )`). */
export const TAB = { friend: 0, guild: 1, campus: 2 } as const;

const ID_SPRITES = 0x7e61;
const ID_TEXTS = 0x7e62;
const ID_PAGE_FRIEND = 0x7e63;
const ID_PAGE_GUILD = 0x7e64;
const SORT_BUTTONS: ReadonlyMap<number, SortKey> = new Map([
  [WIDC_CHANNEL, 'channel'],
  [WIDC_STATE, 'status'],
  [WIDC_LEVEL, 'level'],
  [WIDC_JOB, 'job'],
  [WIDC_NAME, 'name'],
]);

/** The seven entries of the state menu (`m_menuState`): the state each sends and its text. */
export const STATE_MENU: readonly { state: number; tid: number }[] = [
  { state: FRS.ONLINE, tid: TID_FRS_ONLINE_STATUS },
  { state: FRS.ABSENT, tid: TID_FRS_ABSENT },
  { state: FRS.HARDPLAY, tid: TID_FRS_HARDPLAY },
  { state: FRS.EAT, tid: TID_FRS_EAT },
  { state: FRS.REST, tid: TID_FRS_REST },
  { state: FRS.MOVE, tid: TID_FRS_MOVE },
  { state: FRS.OFFLINE, tid: TID_FRS_OFFLINE_STATUS },
];

/** The text under the name: `(%s)` of the five user states, offline and online; the other states draw nothing. */
export function stateStatusText(state: number, getText: (tid: number) => string): string {
  const tid = STATE_MENU.find((e) => e.state === state)?.tid;
  return tid === undefined ? '' : `(${getText(tid)})`;
}

export class WndMessenger extends WndNeuz {
  readonly appId = APP_MESSENGER_EX;
  override readonly processEveryFrame = true;
  readonly #deps: MessengerDeps;
  #tab: TabCtrl | null = null;
  #friend: MessengerListPage | null = null;
  #guild: MessengerListPage | null = null;
  #campus: WndCampusTab | null = null;
  #sprites: SpriteLayer | null = null;
  #texts: TextLayer | null = null;
  #stateMenu: MoverMenu | null = null;
  #friendMenu: MoverMenu | null = null;
  #campusMenu: MoverMenu | null = null;
  #applied: MessengerView = EMPTY_MESSENGER_VIEW;
  #unsubscribe: (() => void) | null = null;
  #flash = 0;
  #flashDir: 1 | -1 = 1;
  #lastProcess: number | null = null;
  #restoreTab = 0;

  constructor(deps: MessengerDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get friendPage(): MessengerListPage | null {
    return this.#friend;
  }
  get guildPage(): MessengerListPage | null {
    return this.#guild;
  }
  get campusPage(): WndCampusTab | null {
    return this.#campus;
  }
  get stateMenu(): MoverMenu | null {
    return this.#stateMenu;
  }
  get friendMenu(): MoverMenu | null {
    return this.#friendMenu;
  }
  get spriteLayer(): SpriteLayer | null {
    return this.#sprites;
  }
  get textLayer(): TextLayer | null {
    return this.#texts;
  }
  /** The `m_nFlashCounter` of the mail picture and the notes button. */
  get flashAlpha(): number {
    return this.#flash;
  }
  /** The selected tab (0 friends, 1 guild, 2 campus). */
  get tabIndex(): number {
    return this.#tab?.getCurSel() ?? this.#restoreTab;
  }
  getButton(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const d = this.#deps;
    const getText = d.getText;
    // the popups of the pages and of the state arrow (`m_menu.CreateMenu( this )`)
    this.#stateMenu = this.#makeMenu();
    this.#friendMenu = this.#makeMenu();
    this.#campusMenu = this.#makeMenu();
    this.#stateMenu.onPick = (state) => {
      d.ports.actions.setState(state);
    };
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    if (tab) {
      this.#tab = tab;
      const friend = new MessengerListPage({
        kind: 'friend',
        actions: d.ports.actions,
        tooltip: d.tooltip,
        menu: this.#friendMenu,
      });
      friend.attach(tab, ID_PAGE_FRIEND);
      const guild = new MessengerListPage({
        kind: 'guild',
        actions: d.ports.actions,
        tooltip: d.tooltip,
        menu: null,
      });
      guild.attach(tab, ID_PAGE_GUILD);
      const campus = new WndCampusTab({
        ports: d.campus,
        tooltip: d.tooltip,
        getText,
        menu: this.#campusMenu,
      });
      campus.attach(tab, APP_MESSENGER_TAB_CAMPUS);
      this.#friend = friend;
      this.#guild = guild;
      this.#campus = campus;
      tab.insertItem(TAB.friend, { text: getText(TID_APP_COMMUNITY_FRIEND), page: friend });
      tab.insertItem(TAB.guild, { text: getText(TID_APP_COMPANY), page: guild });
      tab.insertItem(TAB.campus, { text: getText(TID_APP_COMMUNITY_CAMPUS), page: campus });
      tab.setCurSel(this.#restoreTab);
    }
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.#apply(d.ports.view.get());
    this.#unsubscribe = d.ports.view.subscribe(() => {
      this.#apply(d.ports.view.get());
    });
    // `Move( rectRoot.right - width, rectRoot.bottom - height )`
    const root = this.env.wm.layoutRect;
    const w = rectWidth(this.rectWindow);
    const h = rectHeight(this.rectWindow);
    this.move({ x: root.r - w, y: root.b - h });
    // the open window asks for the states of the friends (a player with the join notices off gets no push)
    d.ports.actions.refreshFriends();
  }

  #makeMenu(): MoverMenu {
    const menu = new MoverMenu();
    menu.attach(this);
    return menu;
  }

  // ---- the view ------------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.ports.view.get();
    if (v !== this.#applied) this.#apply(v);
    const dt = this.#lastProcess === null ? 0 : Math.max(0, now - this.#lastProcess);
    this.#lastProcess = now;
    if (v.mailFlash || v.noteFlash) {
      const next = flashStep(this.#flash, this.#flashDir, dt === 0 ? 17 : dt);
      this.#flash = next.counter;
      this.#flashDir = next.dir;
      this.#paintHeader();
    }
    super.process(now);
  }

  #apply(v: MessengerView): void {
    this.#applied = v;
    this.#friend?.setRows(v.friends);
    this.#guild?.setRows(v.guild);
    this.#paintHeader();
  }

  /** `OnDraw`: the state icon, the texts, the mail picture and the flash of the notes button. */
  #paintHeader(): void {
    const v = this.#applied;
    const getText = this.#deps.getText;
    const sprites: SpriteEntry[] = [];
    const dto = this.#deps.atlases.info(PLAYER_DATA_ATLAS);
    const big = bigStateSprite(v.myState);
    const cut = big === null ? undefined : dto?.sprites[big];
    if (dto && cut) {
      sprites.push({
        key: 'state',
        path: dto.texture,
        x: HEADER.icon.x,
        y: HEADER.icon.y,
        w: cut.w,
        h: cut.h,
        src: { x: cut.x, y: cut.y },
      });
    }
    const alpha = this.#flash / FLASH.max;
    if (v.mailFlash) {
      const at = this.getWndCtrl(WIDC_CUSTOM1)?.rect;
      if (at) {
        sprites.push({
          key: 'mail',
          path: MAIL_PICTURE,
          x: at.l,
          y: at.t,
          w: MAIL_PICTURE_SIZE,
          h: MAIL_PICTURE_SIZE,
          alpha,
        });
      }
    }
    if (v.noteFlash) {
      const at = this.getWndCtrl(WIDC_BUTTON2)?.rect;
      if (at) {
        sprites.push({
          key: 'note',
          path: MAIL_PICTURE,
          x: at.l + 2,
          y: at.t + 2,
          w: MAIL_PICTURE_SIZE,
          h: MAIL_PICTURE_SIZE,
          alpha,
        });
      }
    }
    this.#sprites?.setEntries(sprites);

    const items: TextItem[] = [
      {
        x: HEADER.name.x,
        y: HEADER.name.y,
        text: v.myName,
        color: HEADER_TEXT_COLOR,
      },
      {
        x: HEADER.nameBold.x,
        y: HEADER.nameBold.y,
        text: v.myName,
        color: HEADER_TEXT_COLOR,
      },
      {
        x: HEADER.state.x,
        y: HEADER.state.y,
        text: stateStatusText(v.myState, getText),
        color: HEADER_TEXT_COLOR,
      },
    ];
    if (this.tabIndex === TAB.campus) {
      items.push({
        x: HEADER.campus.x,
        y: HEADER.campus.y,
        text: getText(TID_GAME_CAMPUS_POINTS).replace(/%d/, String(v.campusPoints)),
        color: HEADER_TEXT_COLOR,
      });
    } else {
      items.push({
        x: HEADER.server.x,
        y: HEADER.server.y,
        text: cutServerName(v.serverName),
        color: HEADER_TEXT_COLOR,
      });
    }
    this.#texts?.setItems(items);
  }

  // ---- the tooltip of the mail picture ----------------------------------------------------------------------------------------

  override onMouseMove(pt: { x: number; y: number }): void {
    const at: Rect | undefined = this.getWndCtrl(WIDC_CUSTOM1)?.rect;
    if (
      this.#applied.mailFlash &&
      at &&
      pt.x >= at.l &&
      pt.x < at.r &&
      pt.y >= at.t &&
      pt.y < at.b
    ) {
      const origin = this.getScreenClientRect();
      this.#deps.tooltip.put(
        this,
        this.#deps.getText(TID_MAIL_RECEIVE),
        { l: origin.l + at.l, t: origin.t + at.t, r: origin.l + at.r, b: origin.t + at.b },
        0,
      );
    }
  }

  // ---- the controls ----------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const a = this.#deps.ports.actions;
    if (msg === Wnm.SELCHANGE && id === WIDC_TABCTRL1) {
      this.#paintHeader();
      if (this.tabIndex === TAB.friend) a.refreshFriends();
      else if (this.tabIndex === TAB.campus) this.#campus?.refresh();
      return true;
    }
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_ADD:
          a.openAddFriend();
          return true;
        case WIDC_BUTTON2:
          a.openNotes();
          return true;
        case WIDC_BUTTON1:
          this.#openStateMenu();
          return true;
        default:
          break;
      }
      const key = SORT_BUTTONS.get(id);
      if (key !== undefined) {
        this.sortBy(key);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `SortbyX()` of the page of the selected tab. */
  sortBy(key: SortKey): void {
    const tab = this.tabIndex;
    if (tab === TAB.friend) this.#friend?.sortBy(key);
    else if (tab === TAB.guild) this.#guild?.sortBy(key);
    else this.#campus?.sortBy(key);
  }

  /** The arrow opens the menu under the button (`GetWindowRect( TRUE )`, one button height down). */
  #openStateMenu(): void {
    const menu = this.#stateMenu;
    const button = this.getButton(WIDC_BUTTON1);
    if (!menu || !button) return;
    const r = button.getScreenRect();
    menu.show(
      STATE_MENU.map((e) => ({ id: e.state, text: this.#deps.getText(e.tid) })),
      { x: r.l, y: r.b },
    );
  }

  // ---- `SerializeRegInfo`: the selected tab -------------------------------------------------------------------------------------

  override serializeRegInfo(): unknown {
    return { tab: this.tabIndex };
  }

  override restoreRegInfo(v: unknown): void {
    const tab = (v as { tab?: unknown } | null)?.tab;
    if (typeof tab !== 'number' || tab < 0 || tab > 2 || !Number.isInteger(tab)) return;
    this.#restoreTab = tab;
    this.#tab?.setCurSel(tab);
    this.#paintHeader();
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    super.onDestroy();
  }
}
