// `WndSkillTree` (`CWndSkillTreeEx`, `src:_Interface/WndField.cpp:6541-8750`, ref/22 §17 D, ref/99 §4.6, Plan/42 §19.4 D, 45 §19.1;
// WP-UI-38): `APP_SKILL3` (256 x 624, hotkey K). Four sections one under the other - vagrant, expert, professional and (for masters
// and heroes) hero - each with a header bar that folds it (the up / down arrow at its right end), the skill icons at the places
// of `GetSkillPoint`, a level number on every skill that has levels (a changed level is drawn in the second colour), the selected
// skill enlarged in `WIDC_CUSTOM1`, the skill points in `WIDC_STATIC9` and the buttons `+`, `-`, Reset and Finish.
//
// The window edits nothing itself: the view (`SkillTreeView`) carries the local copy of the 45 slots and everything the rules decide
// (learnable, the enable states of the four buttons), the actions send the user's intent back. The shell owns the layout: the
// section folds (`m_bSlot[0..3]`, persisted with the window rect), the vertical positions of the controls under a folded section,
// the window height, the hit rects, the drag of a skill to the taskbar and the tooltips.

import { JTYPE_HERO } from '../../../gen/defs/job';
import { APP_SKILL3 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { SC } from '../../dnd/shortcut';
import {
  type Point,
  type Rect,
  rectContains,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../../layout/geometry';
import type { WidcKey } from '../../layout/registry';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import { themePath } from '../../skin/images';
import type { BakeSpec } from '../../skin/baker';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { SkillTreeActions, SkillTreeSlot, SkillTreeView, ViewSource } from './ports';
import {
  controlOffsets,
  ICON_SIZE,
  iconRect,
  levelFrame,
  PREVIEW_SIZE,
  sectionGap,
  type SectionFlags,
  type TreeLayout,
  windowHeight,
} from './treeGeometry';

/** The atlas of the level numbers (`icon_IconSkillLevel.inc`: 20 frames per row, the second row for a changed level). */
export const LEVEL_ATLAS = 'icon/icon_IconSkillLevel.inc';
/** The arrows at the right end of a header (`LvUp.bmp` open, `LvDn.bmp` folded; 20 x 20). */
export const ARROW_OPEN = 'theme/lvup.bmp';
export const ARROW_FOLDED = 'theme/lvdn.bmp';
const ARROW_SIZE = 20;

/** The window id of the sprite layer (a code-created control). */
const LAYER_ID = 0x7c10;

export interface SkillTreeDeps {
  readonly view: ViewSource<SkillTreeView>;
  readonly actions: SkillTreeActions;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
}

const HEADERS = ['WIDC_STATIC2', 'WIDC_STATIC3', 'WIDC_STATIC6', 'WIDC_STATIC1'] as const;

export class WndSkillTree extends WndNeuz {
  readonly appId = APP_SKILL3;
  readonly #deps: SkillTreeDeps;
  #layer: SpriteLayer | null = null;
  #unsubscribe: (() => void) | null = null;
  #applied: SkillTreeView | null = null;
  /** `m_bSlot[0..3]`: vagrant, expert, professional, hero. */
  #flags: [boolean, boolean, boolean, boolean] = [true, true, true, true];
  #legend = false;
  #layout: TreeLayout | null = null;
  #layoutHeight = 0;
  #origins = new Map<string, Rect>();
  #tex: { path: string; frames: readonly { x: number; y: number }[] } | null = null;
  /** The skill under the pointer (`m_dwMouseSkill`), `-1` = none. */
  #hover = -1;
  #hoverText = '';
  #panelPaths: readonly [string | null, string | null] = [null, null];

  constructor(deps: SkillTreeDeps) {
    super();
    this.#deps = deps;
  }

  // ---- state for the tests and the probe -----------------------------------------------------------------------------------------

  /** `m_bSlot[0..3]`: which sections are open. */
  get sections(): SectionFlags {
    return this.#flags;
  }
  get layer(): SpriteLayer | null {
    return this.#layer;
  }
  get applied(): SkillTreeView | null {
    return this.#applied;
  }
  get hovered(): number {
    return this.#hover;
  }
  button(n: 1 | 2 | 3 | 4): Button | null {
    return this.getDlgItem<Button>(`WIDC_BUTTON${String(n)}`);
  }
  static(name: string): Static | null {
    return this.getDlgItem<Static>(name as WidcKey);
  }

  // ---- creation ------------------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rectOf = (name: string): Rect => {
      const c = this.getWndCtrl(name as WidcKey);
      return c ? c.rect : { l: 0, t: 0, r: 0, b: 0 };
    };
    for (const n of [
      'WIDC_STATIC1',
      'WIDC_STATIC2',
      'WIDC_STATIC3',
      'WIDC_STATIC4',
      'WIDC_STATIC5',
      'WIDC_STATIC6',
      'WIDC_STATIC7',
      'WIDC_STATIC8',
      'WIDC_STATIC11',
    ]) {
      this.#origins.set(n, rectOf(n));
    }
    this.#layout = {
      expert: { x: rectOf('WIDC_STATIC5').l, y: rectOf('WIDC_STATIC5').t },
      pro: { x: rectOf('WIDC_STATIC7').l, y: rectOf('WIDC_STATIC7').t },
      vagrant: [rectOf('WIDC_CUSTOM2'), rectOf('WIDC_CUSTOM3'), rectOf('WIDC_CUSTOM4')],
      master: rectOf('WIDC_CUSTOM5'),
      hero: rectOf('WIDC_CUSTOM6'),
    };
    this.#layoutHeight = rectHeight(this.rectWindow);
    this.#layer = this.addControl(new SpriteLayer(), LAYER_ID, this.getClientRect(false));
    const v = this.#deps.view.get();
    // the hero section exists for masters and heroes only (`OnInitialUpdate`: `m_bSlot[3] = FALSE` otherwise)
    this.#legend = v.legend;
    this.#flags[3] = v.legend;
    this.#apply(v, true);
    this.#relayout();
    // the view changes only by what the user does or the server sends: it is applied at once (a click on `+` right after the selection
    // of a skill must find the button enabled; the 10 Hz `process` would leave a gap of up to 100 ms)
    this.#unsubscribe = this.#deps.view.subscribe(() => {
      const next = this.#deps.view.get();
      if (next !== this.#applied && !this.destroyed) this.#apply(next, false);
    });
    // the window docks to the bottom right of the layout rect
    const root = this.env.wm.layoutRect;
    const r = this.rectWindow;
    this.move({ x: root.r - rectWidth(r), y: root.b - rectHeight(r) });
  }

  /** The skin pictures under the expert and the professional section (`AfterSkinTexture`). */
  override buildBakeSpec(): BakeSpec | null {
    const base = super.buildBakeSpec();
    if (!base) return base;
    const panels = this.#panelPaths;
    const images = panels.flatMap((p) =>
      p === null ? [] : [{ path: themePath(p), mode: 'nibble' as const }],
    );
    const origins = this.#origins;
    const flags = this.#flags;
    const offset = this.#clientOffset();
    return {
      ...base,
      afterImages: [...(base.afterImages ?? []), ...images],
      after: (ctx) => {
        base.after?.(ctx);
        const offs = controlOffsets(flags);
        const paint = (path: string | null, name: string, dy: number, open: boolean): void => {
          if (path === null || !open) return;
          const o = origins.get(name);
          if (!o) return;
          // `pt = lpWndCtrl->rect.TopLeft() + pt2; pt.y += 2; pt.y += m_nTopDownGap`
          ctx.blit(themePath(path), o.l + offset.x, o.t + offset.y + 2 + dy, 'nibble');
        };
        paint(panels[0], 'WIDC_STATIC5', flags[0] ? 0 : offs.expertHeader, flags[1]);
        paint(panels[1], 'WIDC_STATIC7', offs.proHeader, flags[2]);
      },
    };
  }

  #clientOffset(): Point {
    const w = this.rectWindow;
    const c = this.rectClient;
    return { x: c.l - w.l, y: c.t - w.t };
  }

  // ---- applying the view ---------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v, false);
    this.#refreshTooltip();
    super.process(now);
  }

  #apply(v: SkillTreeView, first: boolean): void {
    const before = this.#applied;
    this.#applied = v;
    if (!first && v.legend !== this.#legend) {
      // the character became (or stopped being) a master / hero: the hero section appears / goes
      this.#legend = v.legend;
      this.#flags[3] = v.legend;
      this.#relayout();
    }
    for (let i = 0; i < 3; i += 1) {
      const name = HEADERS[i];
      if (name) this.static(name)?.setTitle(v.titles[i] ?? '');
    }
    this.static('WIDC_STATIC9')?.setTitle(String(v.sp));
    const b = v.buttons;
    this.button(1)?.enableWindow(b.add);
    this.button(2)?.enableWindow(b.sub);
    this.button(3)?.enableWindow(b.reset);
    this.button(4)?.enableWindow(b.finish);
    if (before?.panels[0] !== v.panels[0] || before.panels[1] !== v.panels[1]) {
      this.#panelPaths = v.panels;
      this.requestBake();
    }
    this.#layer?.setEntries(this.#entries(v));
    if (before !== null && v.job !== before.job) this.#hover = -1;
  }

  /** The sections moved or folded: the controls below move, the window height follows, the frame bakes again. */
  #relayout(): void {
    const v = this.#applied;
    const legend = v?.legend ?? this.#legend;
    const flags = this.#flags as SectionFlags;
    const offs = controlOffsets(flags);
    const place = (name: string, dy: number, visible: boolean): void => {
      const c = this.static(name);
      const o = this.#origins.get(name);
      if (!c || !o) return;
      c.move({ x: o.l, y: o.t + dy });
      c.setVisible(visible);
      c.enableWindow(visible);
    };
    place('WIDC_STATIC4', 0, flags[0]);
    place('WIDC_STATIC8', 0, flags[0]);
    place('WIDC_STATIC3', offs.expertHeader, true);
    place('WIDC_STATIC5', offs.expertHeader, flags[1]);
    place('WIDC_STATIC6', offs.proHeader, true);
    place('WIDC_STATIC7', offs.proHeader, flags[2]);
    place('WIDC_STATIC1', offs.heroHeader, legend);
    place('WIDC_STATIC11', offs.heroHeader, legend && flags[3]);
    // the height of the window; one that would leave the layout rect moves up (`SetActiveSlot`)
    const h = windowHeight(this.#layoutHeight, legend, flags);
    const r = this.rectWindow;
    let top = r.t;
    const bottom = this.env.wm.layoutRect.b;
    if (top + h > bottom) top = Math.max(0, bottom - h);
    this.setWndRect({ l: r.l, t: top, r: r.r, b: top + h }, true);
    this.#layer?.setWndRect(this.getClientRect(false));
    this.requestBake();
  }

  #setSection(n: number, open: boolean): void {
    const cur = this.#flags[n];
    if (cur === undefined || cur === open) return;
    this.#flags[n] = open;
    this.#relayout();
    const v = this.#applied;
    if (v) this.#layer?.setEntries(this.#entries(v));
  }

  // ---- the picture ---------------------------------------------------------------------------------------------------------------

  #texture(): { path: string; frames: readonly { x: number; y: number }[] } | null {
    if (this.#tex) return this.#tex;
    const info = this.#deps.atlases.info(LEVEL_ATLAS);
    if (!info || info.texture === '') return null;
    this.#tex = { path: info.texture, frames: info.sprites.map((s) => ({ x: s.x, y: s.y })) };
    return this.#tex;
  }

  /** The sprites of the whole window, bottom to top (`OnDraw`). */
  #entries(v: SkillTreeView): SpriteEntry[] {
    const layout = this.#layout;
    if (!layout) return [];
    const flags = this.#flags as SectionFlags;
    const out: SpriteEntry[] = [];
    const tex = this.#texture();
    const number = (
      key: string,
      level: number,
      max: number,
      edited: boolean,
      x: number,
      y: number,
    ): void => {
      if (!tex) return;
      const f = tex.frames[levelFrame(level, max, edited)];
      if (!f) return;
      out.push({ key, path: tex.path, x, y, w: 28, h: 28, src: { x: f.x, y: f.y } });
    };
    // the picture under the hero skill
    if (v.legend && flags[3] && v.heroBackground) {
      const r = layout.hero;
      const gap = sectionGap(JTYPE_HERO, flags);
      out.push({
        key: 'hero-bg',
        path: themePath(v.heroBackground),
        x: r.l,
        y: r.t - 2 + gap,
        w: ICON_SIZE,
        h: ICON_SIZE,
      });
    }
    for (const s of v.slots) {
      const r = iconRect(s.skillId, s.kind, layout, flags);
      if (!r || s.icon === null) continue;
      const lit = s.learnable && s.level > 0;
      if (lit) {
        out.push({
          key: `icon:${String(s.index)}`,
          path: s.icon,
          x: r.l,
          y: r.t,
          w: ICON_SIZE,
          h: ICON_SIZE,
        });
        number(
          `lv:${String(s.index)}`,
          s.level,
          s.maxLevel,
          s.level !== s.serverLevel,
          r.l - 2,
          r.t - 2,
        );
      } else if (s.learnable && s.index === this.#hover) {
        out.push({
          key: `icon:${String(s.index)}`,
          path: s.icon,
          x: r.l,
          y: r.t,
          w: ICON_SIZE,
          h: ICON_SIZE,
        });
      }
    }
    // the selected skill enlarged in `WIDC_CUSTOM1`
    const sel = v.slots.find((s) => s.index === v.selected);
    const c1 = this.getWndCtrl('WIDC_CUSTOM1')?.rect;
    if (sel?.icon != null && c1) {
      out.push({
        key: 'preview',
        path: sel.icon,
        x: c1.l,
        y: c1.t,
        w: PREVIEW_SIZE,
        h: PREVIEW_SIZE,
      });
      if (sel.level > 0) {
        number('preview-lv', sel.level, sel.maxLevel, sel.level !== sel.serverLevel, c1.l, c1.t);
      }
    }
    // the fold arrows at the right end of the headers
    HEADERS.forEach((name, i) => {
      if (i === 3 && !v.legend) return;
      const c = this.static(name);
      if (!c) return;
      const r = c.rectWindow;
      out.push({
        key: `arrow:${String(i)}`,
        path: flags[i] ? ARROW_OPEN : ARROW_FOLDED,
        x: r.r - 30,
        y: r.t,
        w: ARROW_SIZE,
        h: ARROW_SIZE,
      });
    });
    return out;
  }

  // ---- pointer -------------------------------------------------------------------------------------------------------------------

  /** The hit rect of a skill icon: `GetSkillPoint` rect, `top - 2`, 27 x 27, plus the gap. */
  #slotRect(s: SkillTreeSlot): Rect | null {
    const layout = this.#layout;
    if (!layout) return null;
    return iconRect(s.skillId, s.kind, layout, this.#flags);
  }

  /** The icon rect of a slot in root (virtual) coordinates, `null` when it is not drawn (a folded section, no place): the probe and the e2e click there. */
  iconScreenRect(index: number): Rect | null {
    const s = this.#applied?.slots.find((x) => x.index === index);
    const r = s ? this.#slotRect(s) : null;
    if (!r) return null;
    const c = this.getScreenClientRect();
    return rectOffset(r, c.l, c.t);
  }

  /** The learnable skill under a client point (`OnLButtonDown`: only `CheckSkill` skills can be pressed), `null` = none. */
  #pick(pt: Point, learnableOnly: boolean): SkillTreeSlot | null {
    const v = this.#applied;
    if (!v) return null;
    for (const s of v.slots) {
      if (learnableOnly && !s.learnable) continue;
      const r = this.#slotRect(s);
      if (r && rectContains(r, pt)) return s;
    }
    return null;
  }

  override onLButtonDown(pt: Point): void {
    const v = this.#applied;
    if (!v) return;
    // the header bars fold their section (`OnLButtonDown`)
    for (let i = 0; i < HEADERS.length; i += 1) {
      if (i === 3 && !v.legend) continue;
      const name = HEADERS[i];
      const c = name ? this.static(name) : null;
      if (c && rectContains(c.rectWindow, pt)) {
        this.#setSection(i, !this.#flags[i]);
        return;
      }
    }
    const s = this.#pick(pt, true);
    if (!s) return;
    this.#deps.actions.select(s.index);
    if (this.env.wm.isDraggingShortcut()) return;
    this.env.wm.drag.arm(this, () => this.#payload(s));
  }

  /** `CWndSkillTreeEx::OnMouseMove`: `SHORTCUT_SKILL`, `m_dwIndex` = the skill id, `m_dwId` = the slot index. */
  #payload(s: SkillTreeSlot): Shortcut {
    return {
      kind: SC.SKILL,
      id: s.index,
      type: 0,
      index: s.skillId,
      userId: 0,
      data: 0,
      ...(s.icon === null ? {} : { icon: s.icon }),
      from: this,
    };
  }

  override onLButtonDblClk(pt: Point): void {
    const s = this.#pick(pt, true);
    if (!s) return;
    this.#deps.actions.select(s.index);
    this.#deps.actions.queue(s.index);
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const s = this.#pick(pt, false);
    const index = s ? s.index : -1;
    if (index === this.#hover) return;
    this.#hover = index;
    this.#hoverText = '';
    const v = this.#applied;
    if (v) this.#layer?.setEntries(this.#entries(v));
    if (!s) {
      this.#deps.tooltip.cancel();
      return;
    }
    this.#showTip(s);
  }

  override onMouseLeave(): void {
    if (this.#hover < 0) return;
    this.#hover = -1;
    this.#hoverText = '';
    const v = this.#applied;
    if (v) this.#layer?.setEntries(this.#entries(v));
  }

  #showTip(s: SkillTreeSlot): void {
    const r = this.#slotRect(s);
    if (!r) return;
    const sc = this.getScreenClientRect();
    this.#hoverText = `${String(s.skillId)}:${String(s.level)}:${String(s.learnable)}`;
    this.#deps.tooltip.putSkill(this, s.skillId, s.level, rectOffset(r, sc.l, sc.t), s.learnable);
  }

  /** The tooltip of the skill under the pointer follows a change of its level (`OnMouseWndSurface` runs every frame). */
  #refreshTooltip(): void {
    if (this.#hover < 0) return;
    const s = this.#applied?.slots.find((x) => x.index === this.#hover);
    if (!s) return;
    const key = `${String(s.skillId)}:${String(s.level)}:${String(s.learnable)}`;
    if (key !== this.#hoverText) this.#showTip(s);
  }

  // ---- commands ------------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const a = this.#deps.actions;
      if (id === this.button(1)?.id) {
        a.add();
        return true;
      }
      if (id === this.button(2)?.id) {
        a.sub();
        return true;
      }
      if (id === this.button(3)?.id) {
        a.reset();
        return true;
      }
      if (id === this.button(4)?.id) {
        a.finish();
        return true;
      }
      if (id === WTBID_CLOSE && this.#deps.actions.close?.()) return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    this.#deps.tooltip.cancel();
    this.#deps.actions.closed?.();
    super.onDestroy();
  }

  // ---- persistence ---------------------------------------------------------------------------------------------------------------

  /** `SerializeRegInfo`: the four section flags. */
  override serializeRegInfo(): unknown {
    return [...this.#flags];
  }

  /** `SerializeRegInfo` on load: `SetActiveSlot(0..2, flag)`, the hero flag only for masters and heroes. */
  override restoreRegInfo(v: unknown): void {
    if (!Array.isArray(v) || v.length !== 4 || !v.every((x) => typeof x === 'boolean')) return;
    const [a, b, c, d] = v as [boolean, boolean, boolean, boolean];
    this.#flags[0] = a;
    this.#flags[1] = b;
    this.#flags[2] = c;
    if (this.#legend) this.#flags[3] = d;
    this.#relayout();
    const view = this.#applied;
    if (view) this.#layer?.setEntries(this.#entries(view));
  }
}
