// `PartySkillPage` (`CWndPartySkill`, `src:_Interface/WndParty.cpp:600-790`, ref/22 §17 H, Plan/42 §19.4 H, 45 §22.2; WP-UI-39): the "Skill" tab of
// `APP_PARTY` (`APP_PARTY_SKILL`). The party skills of `propTroupeSkill` (eight rows, `ST_CALL` .. `ST_GIFTBOX`) in a 3 x 3 grid: the icon at
// the top-left of the cell, the name 35 px to its right. A normal party, or a party below the skill's level, shows the name in red and the
// icon dimmed; an advanced party that lacks the points shows the name in blue; a usable skill is black. A running mode shows the time left
// under the name. Hovering shows the party-skill tooltip, a double click uses the skill, and a press on a cell the party level reaches arms a
// drag: the skill (`SHORTCUT_SKILL`, type 2) can be put on the taskbar.

import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, rectOffset } from '../../layout/geometry';
import { CodePage } from '../char/CodePage';
import { skillAt, skillCell, skillIconPos, skillNamePos, TAB_PAGE_SIZE } from './partyGeometry';
import type { PartyActions, PartySkillRow, PartyView, ViewSource } from './ports';

const NAME_COLOR = { usable: 0xff000000, noPoints: 0xff0000ff, locked: 0xffff0000 } as const;
/** `RenderTexture( point2, tex, 54 )`: the dimmed icon of a locked skill. */
const LOCKED_ALPHA = 54 / 255;
const ID_ICONS = 0x7d21;
const ID_TEXTS = 0x7d22;

export interface PartySkillDeps {
  readonly view: ViewSource<PartyView>;
  readonly actions: PartyActions;
}

export class PartySkillPage extends CodePage {
  readonly #deps: PartySkillDeps;
  #icons: SpriteLayer | null = null;
  #texts: TextLayer | null = null;
  #applied: PartyView | null = null;
  #appliedSize: { w: number; h: number } = { w: 0, h: 0 };
  /** `m_nSkillSelect`, `-1` for none (0-based cell). */
  #selected = -1;
  #hover = -1;

  constructor(deps: PartySkillDeps) {
    super();
    this.#deps = deps;
  }

  get iconLayer(): SpriteLayer | null {
    return this.#icons;
  }
  get textLayer(): TextLayer | null {
    return this.#texts;
  }
  get selected(): number {
    return this.#selected;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.getClientRect(false);
    this.#icons = this.addControl(new SpriteLayer(), ID_ICONS, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.#apply(this.#deps.view.get());
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    // the tab control resizes its pages after they were created (the scroll-bar column): the grid follows
    const size = this.#size();
    if (v !== this.#applied || size.w !== this.#appliedSize.w || size.h !== this.#appliedSize.h) {
      this.#apply(v);
    }
    super.process(now);
  }

  #size(): { w: number; h: number } {
    const r = this.getClientRect(false);
    const w = r.r - r.l;
    const h = r.b - r.t;
    return w > 0 && h > 0 ? { w, h } : TAB_PAGE_SIZE;
  }

  #apply(v: PartyView): void {
    this.#applied = v;
    const { w, h } = this.#size();
    this.#appliedSize = { w, h };
    const sprites: SpriteEntry[] = [];
    const items: TextItem[] = [];
    for (const s of v.skills) {
      const n = s.id - 1;
      const at = skillIconPos(n, w, h);
      sprites.push({
        key: `skill:${String(s.id)}`,
        path: s.icon,
        x: at.x,
        y: at.y,
        w: 32,
        h: 32,
        alpha: s.state === 'locked' ? LOCKED_ALPHA : 1,
      });
      const name = skillNamePos(n, w, h);
      items.push({ x: name.x, y: name.y, text: s.name, color: NAME_COLOR[s.state] });
      if (s.remain !== '') {
        items.push({ x: name.x, y: name.y + 16, text: s.remain, color: 0xff000000 });
      }
    }
    this.#icons?.setEntries(sprites);
    this.#texts?.setItems(items);
  }

  /** The row of the cell at a point (the grid has eight skills; the ninth cell is empty). */
  #skillAt(pt: Point): PartySkillRow | null {
    const { w, h } = this.#size();
    const n = skillAt(pt.x, pt.y, w, h);
    return this.#applied?.skills.find((s) => s.id === n + 1) ?? null;
  }

  override onLButtonDown(pt: Point): void {
    const row = this.#skillAt(pt);
    this.#selected = row ? row.id - 1 : -1;
    if (!row || !row.dragAllowed || this.env.wm.isDraggingShortcut()) return;
    this.env.wm.drag.arm(this, () => {
      const sc: Shortcut = {
        kind: SC.SKILL,
        id: row.id,
        type: 2,
        index: row.id,
        userId: 0,
        data: 0,
        text: row.name,
        ...(row.icon ? { icon: row.icon } : {}),
        from: this,
      };
      return sc;
    });
  }

  override onLButtonUp(): void {
    this.#selected = -1;
  }

  /** `OnLButtonDblClk`: use the skill under the pointer. */
  override onLButtonDblClk(pt: Point): void {
    const row = this.#skillAt(pt);
    if (row) this.#deps.actions.useSkill(row.id);
  }

  /** `OnMouseWndSurface`: the party-skill tooltip of the hovered cell. */
  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const row = this.#skillAt(pt);
    const id = row ? row.id : -1;
    if (id === this.#hover) return;
    this.#hover = id;
    if (!row) {
      this.#deps.actions.skillTooltip(0, null);
      return;
    }
    const { w, h } = this.#size();
    const screen = this.getScreenClientRect();
    this.#deps.actions.skillTooltip(
      row.id,
      rectOffset(skillCell(row.id - 1, w, h), screen.l, screen.t),
    );
  }

  override onMouseLeave(): void {
    this.#hover = -1;
    this.#deps.actions.skillTooltip(0, null);
  }
}
