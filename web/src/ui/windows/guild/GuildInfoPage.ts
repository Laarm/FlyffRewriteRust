// `GuildInfoPage` (`CWndGuildTabInfo`, `src:_Interface/WndGuildTabInfo.cpp:300-619`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-45): the "Information" tab of `APP_GUILD`
// (`APP_GUILD_TABINFO`, 404 x 300). Name, level, master, members "n / max", the contribution points, the guild penya, the notice, the logo (`WIDC_CUSTOM1`, 24 x 24, drawn
// opaque) and four buttons: notice (`WIDC_BUTTON1`), logo (`WIDC_BUTTON2`), disband (`WIDC_BUTTON3`), name (`WIDC_BUTTON4`). FIX: the buttons are enabled for the master only
// (the original left them clickable and returned for everybody else). The window opens blank for a player without a guild and rebuilds on every change of the view
// (`UpdateData`). The checks behind a button (a name is set once, the logo once, no disbanding at war ...) are the feature's.

import {
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_BUTTON4,
  WIDC_CUSTOM1,
  WIDC_GUILDEXPMERIT,
  WIDC_GUILDLEVEL,
  WIDC_GUILDMASTER,
  WIDC_GUILDNAME,
  WIDC_GUILDNUMBER,
  WIDC_GUILDPENYAMERIT,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { CodePage } from '../char/CodePage';
import { guildLogoPath, guildLogoSize } from './logoPath';
import type { GuildActions, GuildView } from './ports';

const ID_LOGO = 0x7f01;

export class GuildInfoPage extends CodePage {
  readonly #actions: GuildActions;
  #logo: SpriteLayer | null = null;
  #view: GuildView | null = null;

  constructor(actions: GuildActions) {
    super();
    this.#actions = actions;
  }

  /** The picture layer of the logo. */
  get logoLayer(): SpriteLayer | null {
    return this.#logo;
  }
  stat(id: number): Static | null {
    return this.getDlgItem<Static>(id);
  }
  get noticeText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  /** `WIDC_BUTTON1` notice, `BUTTON2` logo, `BUTTON3` disband, `BUTTON4` name. */
  button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#logo = this.addControl(new SpriteLayer(), ID_LOGO, this.getClientRect(false));
    if (this.#view) this.apply(this.#view);
  }

  /** `UpdateData`. */
  apply(v: GuildView): void {
    this.#view = v;
    if (!this.#logo) return;
    this.stat(WIDC_GUILDNAME)?.setTitle(v.hasGuild ? v.name : '');
    this.stat(WIDC_GUILDLEVEL)?.setTitle(v.hasGuild ? String(v.level) : '');
    this.stat(WIDC_GUILDMASTER)?.setTitle(v.hasGuild ? v.masterName : '');
    this.stat(WIDC_GUILDNUMBER)?.setTitle(
      v.hasGuild ? `${String(v.memberCount)} / ${String(v.maxMembers)}` : '',
    );
    this.stat(WIDC_GUILDEXPMERIT)?.setTitle(v.hasGuild ? String(v.pxp) : '');
    this.stat(WIDC_GUILDPENYAMERIT)?.setTitle(v.hasGuild ? String(v.penya) : '');
    this.noticeText?.setString(v.hasGuild ? v.notice : '');
    for (const id of [WIDC_BUTTON1, WIDC_BUTTON2, WIDC_BUTTON3, WIDC_BUTTON4]) {
      this.button(id)?.enableWindow(v.hasGuild && v.isMaster);
    }
    const at = this.getWndCtrl(WIDC_CUSTOM1)?.rect;
    const path = v.hasGuild ? guildLogoPath(v.logo) : null;
    const entries: SpriteEntry[] =
      at && path !== null
        ? [
            {
              key: `logo${String(v.logo)}`,
              path,
              x: at.l,
              y: at.t,
              w: guildLogoSize(v.logo),
              h: guildLogoSize(v.logo),
            },
          ]
        : [];
    this.#logo.setEntries(entries);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg !== Wnm.CLICKED) return super.onChildNotify(msg, id, src);
    switch (id) {
      case WIDC_BUTTON1:
        this.#actions.setNotice();
        return true;
      case WIDC_BUTTON2:
        this.#actions.setLogo();
        return true;
      case WIDC_BUTTON3:
        this.#actions.disband();
        return true;
      case WIDC_BUTTON4:
        this.#actions.setName();
        return true;
      default:
        return super.onChildNotify(msg, id, src);
    }
  }
}
