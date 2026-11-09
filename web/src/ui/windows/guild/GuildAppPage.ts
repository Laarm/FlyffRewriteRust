// `GuildAppPage` (`CWndGuildTabApp`, `src:_Interface/WndGuildTabApp.cpp:30-640`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-46): the "Appellation" tab of `APP_GUILD`
// (`APP_GUILD_TABAPPELLATION`, 404 x 304). Per member level (master, kingpin, captain, supporter, rookie): the head count and cap ("%.2d/%.2d", `WIDC_GUILD_NUM_*`), the five
// power check boxes (member level, class, invitation, penya, item: `WIDC_CHECK(r + 5k)` with r = 1 master .. 5 rookie and k = 0..4 for the bits 0x01, 0x02, 0x04, 0x08, 0x10),
// the salary (`WIDC_GUILD_PAY_*`) and the button that opens the salary window of the rank (`WIDC_BUTTON1..5`). The master row of check boxes is always disabled. The five marks
// above the columns are sprites 49..53 of `icon_IconMessenger.inc`. `WIDC_OK` sends the power words.
//
// FIX (the task ruling): the check boxes follow the shared power draft (`PowerView`) both ways - the original's `SetData` only ever checked a box, so a revoked right stayed
// ticked - and the whole page is read-only for everybody but the master. The decisions (what a click changes, what is sent) are the feature's (`actions.setPower`, `sendPower`).

import {
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_BUTTON4,
  WIDC_BUTTON5,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_GUILD_NUM_CAPTAIN,
  WIDC_GUILD_NUM_KINGPIN,
  WIDC_GUILD_NUM_MASTER,
  WIDC_GUILD_NUM_ROOKIE,
  WIDC_GUILD_NUM_SUPPORTER,
  WIDC_GUILD_PAY_CAPTAIN,
  WIDC_GUILD_PAY_KINGPIN,
  WIDC_GUILD_PAY_MASTER,
  WIDC_GUILD_PAY_ROOKIE,
  WIDC_GUILD_PAY_SUPPORTER,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import type { WndBase } from '../../wnd/WndBase';
import { CodePage } from '../char/CodePage';
import { GUILD_RANKS, type GuildActions, type GuildView, PF, type PowerView } from './ports';

const ID_ICONS = 0x7f21;

/** The atlas of the rank marks above the columns (`CWndWorld::m_texMsgIcon`). */
export const MESSENGER_ICON_ATLAS = 'icon/icon_iconmessenger.inc';
/** `43 + rank + 6`: the sprite of a member level. */
export const RANK_MARK_BASE = 49;

/** The bit of each row of check boxes (`k = 0..4`). */
export const APP_BITS: readonly number[] = [
  PF.MEMBERLEVEL,
  PF.LEVEL,
  PF.INVITATION,
  PF.PENYA,
  PF.ITEM,
];

const NUM_IDS = [
  WIDC_GUILD_NUM_MASTER,
  WIDC_GUILD_NUM_KINGPIN,
  WIDC_GUILD_NUM_CAPTAIN,
  WIDC_GUILD_NUM_SUPPORTER,
  WIDC_GUILD_NUM_ROOKIE,
] as const;
const PAY_IDS = [
  WIDC_GUILD_PAY_MASTER,
  WIDC_GUILD_PAY_KINGPIN,
  WIDC_GUILD_PAY_CAPTAIN,
  WIDC_GUILD_PAY_SUPPORTER,
  WIDC_GUILD_PAY_ROOKIE,
] as const;
const PAY_BUTTONS = [WIDC_BUTTON1, WIDC_BUTTON2, WIDC_BUTTON3, WIDC_BUTTON4, WIDC_BUTTON5] as const;
const MARKS = [WIDC_CUSTOM1, WIDC_CUSTOM2, WIDC_CUSTOM3, WIDC_CUSTOM4, WIDC_CUSTOM5] as const;

/** The `WIDC_CHECK` number of a member level (0..4) and a power row (0..4). */
export const appCheckName = (rank: number, row: number): string =>
  `WIDC_CHECK${String(rank + 1 + 5 * row)}`;

export interface AppPageDeps {
  readonly actions: GuildActions;
  readonly atlases: UiAtlases;
}

export class GuildAppPage extends CodePage {
  readonly #deps: AppPageDeps;
  #icons: SpriteLayer | null = null;
  /** Layout control id -> (member level, power bit). */
  readonly #checks = new Map<number, { rank: number; bit: number }>();
  #view: GuildView | null = null;
  #power: PowerView | null = null;

  constructor(deps: AppPageDeps) {
    super();
    this.#deps = deps;
  }

  get iconLayer(): SpriteLayer | null {
    return this.#icons;
  }
  stat(id: number): Static | null {
    return this.getDlgItem<Static>(id);
  }
  /** The check box of a member level (0..4) and a power row (0..4). */
  check(rank: number, row: number): Button | null {
    return this.getDlgItemByName<Button>(appCheckName(rank, row));
  }
  payButton(rank: number): Button | null {
    const id = PAY_BUTTONS[rank];
    return id === undefined ? null : this.getDlgItem<Button>(id);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    for (let rank = 0; rank < GUILD_RANKS; rank += 1) {
      for (let row = 0; row < APP_BITS.length; row += 1) {
        const c = this.check(rank, row);
        const bit = APP_BITS[row];
        if (c && bit !== undefined) this.#checks.set(c.id, { rank, bit });
      }
    }
    this.#icons = this.addControl(new SpriteLayer(), ID_ICONS, this.getClientRect(false));
    this.#paintMarks();
    if (this.#view) this.apply(this.#view);
    if (this.#power) this.applyPower(this.#power);
  }

  /** The five marks above the columns. */
  #paintMarks(): void {
    const dto = this.#deps.atlases.info(MESSENGER_ICON_ATLAS);
    const entries: SpriteEntry[] = [];
    MARKS.forEach((id, rank) => {
      const at = this.getWndCtrl(id)?.rect;
      const cut = dto?.sprites[RANK_MARK_BASE + rank];
      if (!dto || !at || !cut) return;
      entries.push({
        key: `rank${String(rank)}`,
        path: dto.texture,
        x: at.l,
        y: at.t,
        w: cut.w,
        h: cut.h,
        src: { x: cut.x, y: cut.y },
      });
    });
    this.#icons?.setEntries(entries);
  }

  /** `UpdateData`: head counts and salaries. */
  apply(v: GuildView): void {
    this.#view = v;
    if (!this.#icons) return;
    for (let rank = 0; rank < GUILD_RANKS; rank += 1) {
      const num = NUM_IDS[rank];
      const pay = PAY_IDS[rank];
      if (num !== undefined)
        this.stat(num)?.setTitle(v.hasGuild ? (v.rankCounts[rank] ?? '') : '00/00');
      if (pay !== undefined)
        this.stat(pay)?.setTitle(String(v.hasGuild ? (v.salaries[rank] ?? 0) : 0));
    }
  }

  /** `SetData` + `EnableButton`: the boxes show the draft and only the master may touch them (not the master's own row). */
  applyPower(p: PowerView): void {
    this.#power = p;
    if (!this.#icons) return;
    for (let rank = 0; rank < GUILD_RANKS; rank += 1) {
      const word = p.words[rank] ?? 0;
      for (let row = 0; row < APP_BITS.length; row += 1) {
        const c = this.check(rank, row);
        const bit = APP_BITS[row];
        if (!c || bit === undefined) continue;
        c.setCheck((word & bit) !== 0);
        c.enableWindow(p.editable && rank !== 0);
      }
      this.payButton(rank)?.enableWindow(p.editable);
    }
    this.okButton?.enableWindow(p.editable);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const box = this.#checks.get(id);
      if (box) {
        const c = this.getDlgItem<Button>(id);
        if (c) this.#deps.actions.setPower(box.rank, box.bit, c.getCheck() !== 0);
        return true;
      }
      const rank = PAY_BUTTONS.indexOf(id as (typeof PAY_BUTTONS)[number]);
      if (rank >= 0) {
        this.#deps.actions.openPay(rank);
        return true;
      }
      if (id === WIDC_OK) {
        this.#deps.actions.sendPower();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
