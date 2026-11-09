// `CoupleSkillPage` (`CWndCoupleTabSkill`, `src:_Interface/WndField.cpp:23975-24060`, ref/22 §17 K, Plan/42 §19.4 K, 45 §24.2; WP-UI-52): the "Couple Skills" tab of `APP_COUPLE_MAIN`
// (`APP_COUPLE_TAB_SKILL`, 288 x 302). `Bg_Couple_Skill.tga` (96 x 32, three dark cells) lies at the top-left of `WIDC_STATIC1` + (4, 2); the icon of each couple skill item is drawn over its
// 32 x 32 cell when the couple level reaches a level of it (nothing while the player is single: FIX of the original, which indexed level 0 out of range). Every cell shows the item name as
// its tooltip, lit or not. `WIDC_TEXT1` holds the text of `Client/CoupleSkillInfo.inc` (the `coupleskillinfo` panel of the help bundle).

import { APP_COUPLE_TAB_SKILL, WIDC_STATIC1, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Point, Rect } from '../../layout/geometry';
import { rectOffset } from '../../layout/geometry';
import type { TooltipService } from '../../tooltip/TooltipService';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  type CoupleActions,
  type CoupleSkillView,
  EMPTY_COUPLE_SKILL,
  type ViewSource,
} from './ports';

/** The picture of the three dark cells (`m_pSkillBgTexture`). */
export const SKILL_BACKGROUND = 'Bg_Couple_Skill.tga';
/** The cells are 32 x 32 and sit at the group box + (4, 2), 32 px apart. */
export const CELL = 32;
export const CELL_ORIGIN = { x: 4, y: 2 } as const;
/** The tooltip position flag of `PutToolTip( 10000, name, rect, pt, 1 )`. */
const TOOLTIP_POS = 1;
const ID_ICONS = 0x7e71;

/** The rect of cell `i` in the client coordinates of the page, from the layout rect of `WIDC_STATIC1`. */
export function cellRect(group: Rect, i: number): Rect {
  const l = group.l + CELL_ORIGIN.x + CELL * i;
  const t = group.t + CELL_ORIGIN.y;
  return { l, t, r: l + CELL, b: t + CELL };
}

/** The cell under a point of the page (`-1` for none; only the cells of the view count). */
export function cellAt(group: Rect, pt: Point, count: number): number {
  for (let i = 0; i < count; i += 1) {
    const r = cellRect(group, i);
    if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) return i;
  }
  return -1;
}

export interface CoupleSkillDeps {
  readonly view: ViewSource<CoupleSkillView>;
  readonly actions: CoupleActions;
  readonly tooltip: TooltipService;
}

export class CoupleSkillPage extends WndNeuz {
  readonly appId = APP_COUPLE_TAB_SKILL;
  readonly #deps: CoupleSkillDeps;
  #icons: SpriteLayer | null = null;
  #applied: CoupleSkillView | null = null;
  #info: string | null = null;
  #hover = -1;
  #unsubscribe: (() => void) | null = null;

  constructor(deps: CoupleSkillDeps) {
    super();
    this.#deps = deps;
  }

  get iconLayer(): SpriteLayer | null {
    return this.#icons;
  }
  /** `WIDC_TEXT1`. */
  get infoText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#icons = this.addControl(new SpriteLayer(), ID_ICONS, this.getClientRect(false));
    this.#deps.actions.skillPageOpened();
    this.#apply(this.#deps.view.get());
    this.#unsubscribe = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    this.#hoverFromPointer();
    super.process(now);
  }

  /**
   * `OnDraw` read `GetMousePoint()` every frame, so the tooltip did not depend on which control the pointer was over: the plain static behind the cells takes the mouse moves
   * of the web, not the page. The pointer of the window manager is read here instead.
   */
  #hoverFromPointer(): void {
    const wm = this.env.wm;
    const p = wm.lastPointer;
    const group = this.getWndCtrl(WIDC_STATIC1)?.rect;
    let i = -1;
    if (
      p &&
      group &&
      this.isVisible() &&
      wm.hitTest(p).target.getFrameWnd() === this.getFrameWnd()
    ) {
      const sc = this.getScreenClientRect();
      i = cellAt(
        group,
        { x: p.x - sc.l, y: p.y - sc.t },
        (this.#applied ?? EMPTY_COUPLE_SKILL).cells.length,
      );
    }
    this.#setHover(i);
  }

  #apply(v: CoupleSkillView): void {
    this.#applied = v;
    const group = this.getWndCtrl(WIDC_STATIC1)?.rect;
    const entries: SpriteEntry[] = [];
    if (group) {
      const first = cellRect(group, 0);
      entries.push({
        key: 'background',
        path: SKILL_BACKGROUND,
        x: first.l,
        y: first.t,
        w: CELL * 3,
        h: CELL,
      });
      v.cells.forEach((c, i) => {
        if (!c.lit || c.icon === null) return;
        const r = cellRect(group, i);
        entries.push({ key: `skill:${String(i)}`, path: c.icon, x: r.l, y: r.t, w: CELL, h: CELL });
      });
    }
    this.#icons?.setEntries(entries);
    // the text is set once per change, so that a scrolled panel keeps its position
    if (this.#info !== v.info) {
      this.#info = v.info;
      this.infoText?.setParsingString(v.info);
    }
  }

  /** The mouse moves that reach the page itself (the cells between the statics). */
  override onMouseMove(pt: Point): void {
    const group = this.getWndCtrl(WIDC_STATIC1)?.rect;
    this.#setHover(
      group ? cellAt(group, pt, (this.#applied ?? EMPTY_COUPLE_SKILL).cells.length) : -1,
    );
  }

  /** `PutToolTip( 10000, strSkillName, rect, ptMouse, 1 )` for the cell under the pointer; the tooltip service itself drops it when the pointer leaves the cell. */
  #setHover(i: number): void {
    if (i === this.#hover) return;
    this.#hover = i;
    const group = this.getWndCtrl(WIDC_STATIC1)?.rect;
    const cell = (this.#applied ?? EMPTY_COUPLE_SKILL).cells[i];
    if (!group || !cell) return;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.put(this, cell.name, rectOffset(cellRect(group, i), s.l, s.t), TOOLTIP_POS);
  }

  override onMouseLeave(): void {
    this.#hover = -1;
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    super.onDestroy();
  }
}
