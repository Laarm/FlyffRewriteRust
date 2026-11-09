// `GuildWarPage` (`CWndGuildTabWar`, `src:_Interface/WndGuildTabWar.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-47): the war tab of `APP_GUILD` (`APP_GUILD_TABGUILDWAR`,
// 404 x 300). The enemy guild (name and logo at `WIDC_CUSTOM1`), the won / lost / surrendered counters of the OWN guild, the time left of the running war (`startAt + 7200 s`
// minus now, "HH:MM:SS", recomputed every process, empty without a war) and three buttons: declare (`WIDC_BUTTON1`, master), truce (`WIDC_BUTTON2`, master), surrender
// (`WIDC_BUTTON3`, every member). FIX: the master-only buttons are disabled for everybody else (the original returned silently); the original's NULL dereferences of an enemy
// guild that is not known (`Error( "..." )` and a return) draw nothing here.

import {
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_CUSTOM1,
  WIDC_GUILDWARGIVEUP,
  WIDC_GUILDWARGUILDNAME,
  WIDC_GUILDWARLEFTTIME,
  WIDC_GUILDWARLOSE,
  WIDC_GUILDWARWIN,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { CodePage } from '../char/CodePage';
import { guildLogoPath, guildLogoSize } from './logoPath';
import type { GuildActions, GuildView } from './ports';

const ID_LOGO = 0x7f31;
/** `CGuildWar::GetEndTime`: a war lasts two hours. */
export const WAR_SECONDS = 7200;

/** `CTimeSpan::Format( "%H:%M:%S" )` of the time left (never below zero). */
export function formatLeft(startAt: number, nowSec: number): string {
  const left = Math.max(0, Math.floor(startAt + WAR_SECONDS - nowSec));
  const h = Math.floor(left / 3600) % 24;
  const m = Math.floor(left / 60) % 60;
  const s = left % 60;
  const two = (n: number): string => String(n).padStart(2, '0');
  return `${two(h)}:${two(m)}:${two(s)}`;
}

export interface WarPageDeps {
  readonly actions: GuildActions;
  /** Unix seconds. */
  readonly now: () => number;
}

export class GuildWarPage extends CodePage {
  readonly #deps: WarPageDeps;
  #logo: SpriteLayer | null = null;
  #view: GuildView | null = null;
  #left = '';

  constructor(deps: WarPageDeps) {
    super();
    this.#deps = deps;
  }

  get logoLayer(): SpriteLayer | null {
    return this.#logo;
  }
  stat(id: number): Static | null {
    return this.getDlgItem<Static>(id);
  }
  button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }
  /** The text of `WIDC_GUILDWARLEFTTIME`. */
  get leftText(): string {
    return this.#left;
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
    this.stat(WIDC_GUILDWARGUILDNAME)?.setTitle(v.hasGuild ? v.war.enemyName : '');
    this.stat(WIDC_GUILDWARWIN)?.setTitle(v.hasGuild ? v.war.winText : '');
    this.stat(WIDC_GUILDWARLOSE)?.setTitle(v.hasGuild ? v.war.loseText : '');
    this.stat(WIDC_GUILDWARGIVEUP)?.setTitle(v.hasGuild ? v.war.surrenderText : '');
    this.button(WIDC_BUTTON1)?.enableWindow(v.hasGuild && v.isMaster);
    this.button(WIDC_BUTTON2)?.enableWindow(v.hasGuild && v.isMaster);
    this.button(WIDC_BUTTON3)?.enableWindow(v.hasGuild);
    const at = this.getWndCtrl(WIDC_CUSTOM1)?.rect;
    const path = v.hasGuild && v.war.startAt !== null ? guildLogoPath(v.war.enemyLogo) : null;
    const entries: SpriteEntry[] =
      at && path !== null
        ? [
            {
              key: `enemy${String(v.war.enemyLogo)}`,
              path,
              x: at.l,
              y: at.t,
              w: guildLogoSize(v.war.enemyLogo),
              h: guildLogoSize(v.war.enemyLogo),
            },
          ]
        : [];
    this.#logo.setEntries(entries);
    this.#tick();
  }

  /** `OnDraw`: the time left. */
  #tick(): void {
    const v = this.#view;
    const start = v?.war.startAt ?? null;
    const text = start !== null ? formatLeft(start, this.#deps.now()) : '';
    this.#left = text;
    this.stat(WIDC_GUILDWARLEFTTIME)?.setTitle(text);
  }

  override process(now: number): void {
    if (this.#view) this.#tick();
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const a = this.#deps.actions;
      switch (id) {
        case WIDC_BUTTON1:
          a.declareWar();
          return true;
        case WIDC_BUTTON2:
          a.truce();
          return true;
        case WIDC_BUTTON3:
          a.surrender();
          return true;
        default:
          break;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
