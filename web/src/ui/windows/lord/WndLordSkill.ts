// `WndLordSkill` (`CWndLordSkill`, `src:_Interface/WndLord.cpp:750-903`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-54): `APP_LORD_SKILL` (728, 192 x 272, "Lord Skill"), the APPLET of the lord (hotkey L): the
// eight lord skills as 32 x 32 icons on the placeholders `CUSTOM1..8` (`CUSTOM9` is in the layout and was stored but never drawn or hit: ignored), a text line above them (`TID_GAME_LORD_SKILL_INFO`).
// A character that is not the lord gets the chat line `TID_GAME_L_EVENT_CREATE_E001` and NO window: the applet factory cannot refuse, so the window destroys itself before its first paint.
// A press on an icon arms the drag of a `SHORTCUT_LORDSKILL` with `id = index + 1` (the window's ids are 1-based so that skill 0 never looks empty; the taskbar stores `id - 1` = the `LI_*` id and
// adds one again on a drag out: KEEP-QUIRK, OQ7); hovering shows `"#b#cff2fbe6d<name>#nb#nc \n<desc>"`; a double click uses the skill (an addition of 45 §28.2: the original had the drag only).
// A plain class: the applet registry builds it synchronously.

import {
  APP_LORD_SKILL,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
  WIDC_CUSTOM7,
  WIDC_CUSTOM8,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { TID_GAME_L_EVENT_CREATE_E001, TID_GAME_LORD_SKILL_INFO } from '../../../gen/defs/text';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { parseMarkup } from '../../text/markup';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LordSkillRow, LordView, LordWindowDeps } from './ports';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_SPRITES = 0x7c30;
/** The size of an icon (`Render( p2DRender, rect.TopLeft(), CPoint( 32, 32 ) )`). */
export const SKILL_ICON = 32;
const CELLS = [
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
  WIDC_CUSTOM7,
  WIDC_CUSTOM8,
] as const;

export interface LordSkillDeps extends LordWindowDeps {
  readonly tooltip: TooltipService;
}

/** The window tooltip: the name in bold green, a blank, a line break, the description (`"#b#cff2fbe6d%s#nb#nc \n%s"`). */
export function skillWindowTooltip(skill: Pick<LordSkillRow, 'name' | 'desc'>): string {
  return `#b#cff2fbe6d${skill.name}#nb#nc \n${skill.desc}`;
}

export class WndLordSkill extends WndNeuz {
  readonly appId = APP_LORD_SKILL;
  readonly #deps: LordSkillDeps;
  #cells: Rect[] = [];
  #sprites: SpriteLayer | null = null;
  #view: LordView | null = null;
  #hover = -1;

  constructor(deps: LordSkillDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get sprites(): SpriteLayer | null {
    return this.#sprites;
  }
  get infoText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  /** The rectangle of the icon `index` (0..7) in client coordinates. */
  cellRect(index: number): Rect {
    return this.#cells[index] ?? NO_RECT;
  }
  /** The cell under a client point, -1 for none. */
  cellAt(pt: Point): number {
    return this.#cells.findIndex((r) => rectContains(r, pt));
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const view = this.#deps.ports.view.get();
    // `Initialize`: only the lord gets the window (the applet factory cannot refuse, so the window goes before its first paint)
    if (!view.isLord) {
      this.#deps.ports.say(TID_GAME_L_EVENT_CREATE_E001);
      this.destroy();
      return;
    }
    this.infoText?.setString(this.#deps.host.text(TID_GAME_LORD_SKILL_INFO));
    this.#cells = CELLS.map((id) => this.getWndCtrl(id)?.rect ?? NO_RECT);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, this.getClientRect(false));
    this.moveParentCenter();
    this.#apply(view);
  }

  #apply(view: LordView): void {
    this.#view = view;
    const entries: SpriteEntry[] = [];
    this.#cells.forEach((r, i) => {
      const skill = view.skills[i];
      if (!skill?.icon) return;
      entries.push({
        key: `lord${String(i)}`,
        path: skill.icon,
        x: r.l,
        y: r.t,
        w: SKILL_ICON,
        h: SKILL_ICON,
      });
    });
    this.#sprites?.setEntries(entries);
  }

  override process(now: number): void {
    const view = this.#deps.ports.view.get();
    // a character that lost the lordship while the window was open is handled by the feature (`DoEndVoteComplete` closes it)
    if (view !== this.#view) this.#apply(view);
    super.process(now);
  }

  /** `OnLButtonDown`: a press on an icon arms the drag (`m_nCurSelect = i + 1`). */
  override onLButtonDown(pt: Point): void {
    const index = this.cellAt(pt);
    const skill = this.#deps.ports.view.get().skills[index];
    if (index < 0 || !skill || this.env.wm.isDraggingShortcut()) return;
    this.env.wm.drag.arm(this, () => this.dragPayload(index));
  }

  /** `OnMouseMove`: the `SHORTCUT_LORDSKILL` shortcut; `id` is the 1-based cell (`m_dwId = m_nCurSelect`). */
  dragPayload(index: number): Shortcut | null {
    const skill = this.#deps.ports.view.get().skills[index];
    if (!skill) return null;
    return {
      kind: SC.LORDSKILL,
      id: index + 1,
      type: 0,
      index: 0,
      userId: 0,
      data: 0,
      ...(skill.icon ? { icon: skill.icon } : {}),
      from: this,
    };
  }

  /** The double click uses the skill. */
  override onLButtonDblClk(pt: Point): void {
    const index = this.cellAt(pt);
    if (index >= 0 && this.#deps.ports.view.get().skills[index]) {
      this.#deps.ports.actions.castSkill(index);
    }
  }

  /** `OnMouseWndSurface`: the tooltip of the icon under the pointer. */
  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const index = this.cellAt(pt);
    if (index === this.#hover) return;
    this.#hover = index;
    const skill = this.#deps.ports.view.get().skills[index];
    if (!skill) return;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.put(
      this,
      parseMarkup(skillWindowTooltip(skill)),
      rectOffset(this.cellRect(index), s.l, s.t),
      0,
    );
  }

  override onMouseLeave(): void {
    this.#hover = -1;
  }
}
