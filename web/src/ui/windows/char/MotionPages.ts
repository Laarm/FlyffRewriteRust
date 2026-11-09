// The two icon pages of the motion window (`CWndMotion1`, `CWndEmoticon`, `src:_Interface/WndMotion.cpp:96-453`; Plan/42 §19.4 D,
// 45 §19.4; WP-UI-21): 32 x 32 icons in 8 columns at `(col * 32 + 3, 7 + row * 32)`. A press selects the icon under the
// pointer, the release inside the window executes it (a single click) and a drag from a selected icon starts a
// `SHORTCUT_MOTION` / `SHORTCUT_EMOTICON` drag for the taskbar. Hovering shows the description (`"/" + command` for emoticons).
//
// The motion icons are item-style icon files drawn by a `PaperDoll` layer; the emoticons are cut out of the sprite sheet
// `Theme/texChatEmoticon.inc` (the atlas, drawn on a canvas once the sprites are loaded).

import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import type { UiAtlases, AtlasSpriteRef } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WidcKey } from '../../layout/registry';
import { CodePage } from './CodePage';
import { PaperDoll, type PaperLayer } from './PaperDoll';
import type { EmoticonEntry, MotionActions, MotionEntry, MotionView, ViewSource } from './ports';

/** The grid of both pages: 8 columns of 32 px, the first icon at (3, 7). */
export const MOTION_COLUMNS = 8;
export const MOTION_CELL = 32;
export const MOTION_ORIGIN = { x: 3, y: 7 } as const;

/** The rect of the icon at a list position. */
export function motionCell(index: number): Rect {
  const row = Math.floor(index / MOTION_COLUMNS);
  const col = index - row * MOTION_COLUMNS;
  const x = col * MOTION_CELL + MOTION_ORIGIN.x;
  const y = MOTION_ORIGIN.y + row * MOTION_CELL;
  return { l: x, t: y, r: x + MOTION_CELL, b: y + MOTION_CELL };
}

/** The list position under a point of the page, or `-1`. */
export function motionCellAt(pt: Point, count: number): number {
  for (let i = 0; i < count; i += 1) {
    if (rectContains(motionCell(i), pt)) return i;
  }
  return -1;
}

export interface MotionPageDeps {
  readonly view: ViewSource<MotionView>;
  readonly actions: MotionActions;
  readonly tooltip: TooltipService;
}

/** `CWndMotion1`: one icon per `MotionProp`. */
export class MotionPage extends CodePage {
  readonly #deps: MotionPageDeps;
  #icons: PaperDoll | null = null;
  #applied: MotionView | null = null;
  #motions: readonly MotionEntry[] = [];
  /** `m_pSelectMotion` as a list position, `-1` = none. */
  #selected = -1;
  #hover = -1;
  #hoverText = '';

  constructor(deps: MotionPageDeps) {
    super();
    this.#deps = deps;
  }

  get iconLayer(): PaperDoll | null {
    return this.#icons;
  }
  get selected(): number {
    return this.#selected;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#icons = this.addControl(new PaperDoll(), 0x7c01, this.getClientRect(false));
    this.#apply(this.#deps.view.get());
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    this.#refreshTooltip();
    super.process(now);
  }

  #apply(v: MotionView): void {
    this.#applied = v;
    this.#motions = v.motions;
    const layers: PaperLayer[] = v.motions.map((m, i) => {
      const c = motionCell(i);
      return {
        key: `motion:${String(m.id)}`,
        x: c.l,
        y: c.t,
        size: 32,
        icon: m.icon,
        alpha: 1,
        tint: false,
      };
    });
    this.#icons?.setLayers(layers, true);
  }

  /** The tooltip of the icon under the pointer follows its text (the cheer motion adds live lines). */
  #refreshTooltip(): void {
    if (this.#hover < 0) return;
    const entry = this.#motions[this.#hover];
    if (!entry) return;
    const text = this.#deps.view.get().tooltip(entry);
    if (text === this.#hoverText) return;
    this.#showTip(this.#hover, text);
  }

  #showTip(index: number, text: string): void {
    this.#hoverText = text;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.put(this, text, rectOffset(motionCell(index), s.l, s.t), 0);
  }

  override onLButtonDown(pt: Point): void {
    this.#selected = motionCellAt(pt, this.#motions.length);
    const entry = this.#motions[this.#selected];
    if (!entry || this.env.wm.isDraggingShortcut()) return;
    const index = this.#selected;
    this.env.wm.drag.arm(this, () => this.#payload(index));
  }

  /** `CWndMotion1::OnMouseMove`: `SHORTCUT_MOTION`, `index` = the list position, `id` = `dwID`, the icon. */
  #payload(index: number): Shortcut | null {
    const m = this.#motions[index];
    if (!m) return null;
    return {
      kind: SC.MOTION,
      id: m.id,
      type: 0,
      index,
      userId: 0,
      data: 1,
      icon: m.icon,
      from: this,
    };
  }

  /** `OnLButtonUp`: the release executes the selected motion (single click). */
  override onLButtonUp(): void {
    const m = this.#motions[this.#selected];
    if (m) this.#deps.actions.execute(m.id);
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const i = motionCellAt(pt, this.#motions.length);
    if (i === this.#hover) return;
    this.#hover = i;
    const entry = this.#motions[i];
    if (entry) this.#showTip(i, this.#deps.view.get().tooltip(entry));
  }

  override onMouseLeave(): void {
    this.#hover = -1;
    this.#hoverText = '';
  }
}

export interface EmoticonPageDeps {
  readonly view: ViewSource<MotionView>;
  readonly actions: MotionActions;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
}

/** `CWndEmoticon`: the user emoticons of the chat, from the sprite sheet. */
export class EmoticonPage extends CodePage {
  readonly #deps: EmoticonPageDeps;
  #emoticons: readonly EmoticonEntry[] = [];
  #sprites: (AtlasSpriteRef | undefined)[] = [];
  #loaded = false;
  /** `m_nSelect`, `-1` = none. */
  #selected = -1;
  #hover = -1;
  #gen = 0;

  constructor(deps: EmoticonPageDeps) {
    super();
    this.#deps = deps;
  }

  get selected(): number {
    return this.#selected;
  }
  /** The sprites were loaded and drawn (tests, screenshots). */
  get loaded(): boolean {
    return this.#loaded;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.getClientRect(false);
    this.addCustomCanvas('emoticons', r);
    this.#emoticons = this.#deps.view.get().emoticons;
    void this.#loadSprites();
  }

  async #loadSprites(): Promise<void> {
    const gen = (this.#gen += 1);
    const list = this.#emoticons;
    const sprites = await Promise.all(
      list.map((e) => this.#deps.atlases.sprite(e.atlas, e.sprite)),
    );
    if (gen !== this.#gen || this.destroyed) return;
    this.#sprites = sprites;
    this.#loaded = true;
    this.invalidate();
  }

  override process(now: number): void {
    const e = this.#deps.view.get().emoticons;
    if (e !== this.#emoticons) {
      this.#emoticons = e;
      this.#loaded = false;
      void this.#loadSprites();
    }
    super.process(now);
  }

  /** `OnDraw`: each loaded sprite at its cell. */
  override onDraw(_id: WidcKey, ctx: CanvasRenderingContext2D): void {
    this.#sprites.forEach((s, i) => {
      if (!s) return;
      const c = motionCell(i);
      const img = new ImageData(new Uint8ClampedArray(s.img.data), s.img.width, s.img.height);
      ctx.putImageData(img, c.l - s.sx, c.t - s.sy, s.sx, s.sy, s.w, s.h);
    });
  }

  override onLButtonDown(pt: Point): void {
    this.#selected = motionCellAt(pt, this.#emoticons.length);
    const e = this.#emoticons[this.#selected];
    if (!e || this.env.wm.isDraggingShortcut()) return;
    const index = this.#selected;
    this.env.wm.drag.arm(this, () => this.#payload(index));
  }

  /** `CWndEmoticon::OnMouseMove`: `SHORTCUT_EMOTICON`, `index` = `id` = the list position, the text `"/" + command`. */
  #payload(index: number): Shortcut | null {
    const e = this.#emoticons[index];
    if (!e) return null;
    return {
      kind: SC.EMOTICON,
      id: index,
      type: 0,
      index,
      userId: 0,
      data: 1,
      text: `/${e.command}`,
      from: this,
    };
  }

  /** `OnLButtonUp`: executes the selected emoticon (FIX: nothing when the press was on no icon). */
  override onLButtonUp(): void {
    if (this.#selected >= 0 && this.#emoticons[this.#selected]) {
      this.#deps.actions.sendEmoticon(this.#selected);
    }
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const i = motionCellAt(pt, this.#emoticons.length);
    if (i === this.#hover) return;
    this.#hover = i;
    const e = this.#emoticons[i];
    if (!e) return;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.put(this, `/${e.command}`, rectOffset(motionCell(i), s.l, s.t), 0);
  }

  override onMouseLeave(): void {
    this.#hover = -1;
  }
}
