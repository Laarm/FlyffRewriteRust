// Ports of the skill-tree window shells (Plan/42 §19.4 D, WP-UI-38; wired by 45 WP-CFEAT-31): `APP_SKILL3` reads a view that the
// feature computes from its store (the local copy of the 45 slots and everything the rules decide: learnable, the enable states of
// the four buttons) and calls plain actions; `APP_RESKILL_CONTROL1` and `APP_HERO_SKILLUP` take one request object each. What the
// actions send (`DoUseSkillPointReq`, `LegendSkillUpStart`) is 45's; the shells keep the control behaviour of the original
// (`src:_Interface/WndField.cpp:6541-8750`, :14097-14290, :21611-21915).

export type { ViewSource } from '../login/ports';

/** One skill of the tree as the window draws it. */
export interface SkillTreeSlot {
  /** The index in the 45 job-skill slots (`g_nSkillCurSelect`, the `id` of a dragged shortcut). */
  readonly index: number;
  readonly skillId: number;
  /** `dwItemKind1`: `JTYPE_BASE` / `JTYPE_EXPERT` / `JTYPE_PRO` / `JTYPE_MASTER` / `JTYPE_HERO`. */
  readonly kind: number;
  /** `icon/<file>` of the skill; `null` = no picture. */
  readonly icon: string | null;
  /** The level in the local copy. */
  readonly level: number;
  /** The level the server knows (`g_pPlayer->GetSkill(...)->dwLevel`). */
  readonly serverLevel: number;
  /** `dwExpertMax`. */
  readonly maxLevel: number;
  /** `CheckSkill(i)`: lit, selectable, draggable. */
  readonly learnable: boolean;
}

/** What the four buttons may do (`CWndSkillTreeEx::Process`). */
export interface SkillTreeButtons {
  readonly add: boolean;
  readonly sub: boolean;
  readonly reset: boolean;
  readonly finish: boolean;
}

/** The read model of `APP_SKILL3`. */
export interface SkillTreeView {
  /** The job of the character (`JOB_*`); `-1` before the character is known. */
  readonly job: number;
  /** `m_bLegend`: the hero section exists (master and hero characters). */
  readonly legend: boolean;
  /** The captions of the three tier headers (`WIDC_STATIC2`, `WIDC_STATIC3`, `WIDC_STATIC6`): the job names, `''` = none. */
  readonly titles: readonly [string, string, string];
  /** The pictures under the expert and the professional section (`Back_Me.TGA`, ...): theme file names; `null` = none. */
  readonly panels: readonly [string | null, string | null];
  /** The picture under the hero skill (`Back_Hero_*.tga`); `null` = none. */
  readonly heroBackground: string | null;
  /** The skills that have a place in the window and a prop (the others are not drawn). */
  readonly slots: readonly SkillTreeSlot[];
  /** `m_nCurrSkillPoint`, shown in `WIDC_STATIC9`. */
  readonly sp: number;
  /** `g_pPlayer->m_nSkillPoint`. */
  readonly serverSp: number;
  /** `g_nSkillCurSelect`, `-1` = none. */
  readonly selected: number;
  readonly buttons: SkillTreeButtons;
  /** The level-up marks: the character level (`GetLevel()`), for the tooltips. */
  readonly level: number;
}

/** The actions of `APP_SKILL3`. */
export interface SkillTreeActions {
  /** A press on a learnable skill (`g_nSkillCurSelect = i`, `m_pFocusItem`). */
  select(index: number): void;
  /** `WIDC_BUTTON1`. */
  add(): void;
  /** `WIDC_BUTTON2`. */
  sub(): void;
  /** `WIDC_BUTTON3`. */
  reset(): void;
  /** `WIDC_BUTTON4`: opens the re-skill warning (the send waits for its Yes). */
  finish(): void;
  /** Double click on a learnable skill: `SetSkillQueue` of the taskbar (only while no skill queue runs). */
  queue(index: number): void;
  /**
   * The close button of the title bar (`nID == 10000`): with unspent edits the window does not close, the re-skill warning opens
   * instead (and closes the tree with it); `true` = handled, `false`/absent = close as usual.
   */
  close?(): boolean;
  /** The window closed (by any route): the feature forgets the instance. */
  closed?(): void;
}

/** `CWndReSkillControl` (`APP_RESKILL_CONTROL1`, "Skill Distribution"): the help text and two buttons. */
export interface ReSkillControlRequest {
  /** `TID_RESKILLPOINT_CONTROL1`. */
  readonly text: string;
  /** `WIDC_BUTTON1`: reload the skill tree from the server state (`InitItem(..., TRUE)`). */
  reset(): void;
  /** `WIDC_BUTTON2`: opens the warning (`CWndReSkillWarning`). */
  finish(): void;
  closed?(): void;
}

/** `CWndHeroSkillUp` (`APP_HERO_SKILLUP`): five gems, one per slot, then OK. */
export interface HeroSkillUpRequest {
  /** The five gem item ids in slot order: diamond, emerald, sapphire, ruby, topaz. */
  readonly jewelIds: readonly [number, number, number, number, number];
  /** `item/<icon>` of each default gem picture (`WIDC_CUSTOM1..5`). */
  readonly jewelIcons: readonly (string | null)[];
  /** The description (`HeroSkillUp.inc`), `''` = none. */
  readonly description: string;
  /** The item id of an inventory object (`undefined` = no such object). */
  itemId(objId: number): number | undefined;
  /** `item/<icon>` of an inventory object (`null` = no picture). */
  itemIcon(objId: number): string | null;
  /** An item the window holds becomes reserved (`SetExtra(+1)`), released again (`SetExtra(0)`). */
  reserve(objId: number, on: boolean): void;
  /** OK with all five gems: `LegendSkillUpStart{ids[5]}` (sent once). */
  start(objIds: readonly number[]): void;
  /** `LegendSkillUpResult`: -1 the skill is at its maximum, 0 failed, 1 succeeded (a message box; the window closes after it). */
  result?(result: number): void;
  /** The window went away: the reservations are released by the window itself. */
  closed?(): void;
}
