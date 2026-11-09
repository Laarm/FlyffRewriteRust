// `WndTele16` (`CWndTeleporter`, `src:_Interface/WndField.cpp:26711-26940`; ref/22 §17 S, Plan/42 WP-UI-36, 45 §26, 15 §10.7):
// `APP_TELE16` (hotkey V, `features.teleportation_system`), the teleporter of the repack. A list of the named rows of `teleport16.inc`,
// the description and the picture of the selected row and the Teleport button; a row with a boss widens the window by 190 px and shows
// the boss panel (name, level, health, element, level requirement, cooldown) and a scrolling grid of the boss drops with item tooltips.
//
// The shell only shows the entries it is given (the texts and the row index are the feature's, Plan/45 §26 "FIX": the index sent is the
// ROW index of the data, not the position in the list - they differ when a row has an empty name). Teleport sends `Tele16{index}` and
// closes the window.

import {
  APP_TELE16,
  WIDC_BUTTON,
  WIDC_CUSTOM,
  WIDC_LISTBOX,
  WIDC_STATIC1,
  WIDC_TEXT,
} from '../../../gen/defs/resdata';
import type { ListBox } from '../../controls/ListBox';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import {
  type Point,
  type Rect,
  rectContains,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { Tele16Boss, Tele16Entry, Tele16Ports } from './ports';

/** The boss panel widens the window by 190 px. */
export const TELE16_BOSS_WIDTH = 190;
/** `nIconSize`: the cell of the drop grid; the icon (32 px) is drawn at `30 / 35`. */
export const DROP_CELL = 30;
const DROP_ICON = Math.round((32 * 30) / 35);
/** The picture sits 20 px below the top of `WIDC_STATIC1`; the default picture of a missing file. */
export const PIC_DROP = 20;
export const DEFAULT_PICTURE = 'tp_default.png';
/** The rows of the panel (`TextOut( clientW - 10, y )`): name, level, health, element, requirement, cooldown. */
export const PANEL_ROWS = [19, 49, 69, 89, 109, 129] as const;
const BLACK = 0xff000000;

export interface Tele16Deps {
  readonly ports: Tele16Ports;
  readonly tooltip: Pick<TooltipService, 'putItem' | 'cancel'>;
  /** The sprite cut of the element icon `i` (`icon/icon_monelemantkind.dds`). */
  readonly elementCut?: (i: number) => { x: number; y: number };
  readonly elementPath?: string;
}

export class WndTele16 extends WndNeuz {
  readonly appId = APP_TELE16;
  readonly #deps: Tele16Deps;
  #list: ListBox | null = null;
  #text: TextCtrl | null = null;
  #pic: Static | null = null;
  #sprites!: SpriteLayer;
  #texts!: TextLayer;
  #entries: readonly Tele16Entry[] = [];
  #sel = -1;
  #defaultW = 0;
  #defaultClientW = 0;
  #dropRect: Rect | null = null;
  #dropScroll = 0;
  #dropHover = -1;

  constructor(deps: Tele16Deps) {
    super();
    this.#deps = deps;
  }

  get list(): ListBox | null {
    return this.#list;
  }
  /** The row of the data that the selected list entry stands for (what `Tele16` carries), `-1` for none. */
  get selectedIndex(): number {
    return this.#entries[this.#sel]?.index ?? -1;
  }
  /** The selected position in the list. */
  get selected(): number {
    return this.#sel;
  }
  get entries(): readonly Tele16Entry[] {
    return this.#entries;
  }
  get spriteLayer(): SpriteLayer {
    return this.#sprites;
  }
  get textLayer(): TextLayer {
    return this.#texts;
  }
  /** The width of the window without the boss panel. */
  get defaultWidth(): number {
    return this.#defaultW;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#defaultW = rectWidth(this.rectWindowValue);
    this.#defaultClientW = rectWidth(this.getClientRect(false));
    this.#list = this.getDlgItem<ListBox>(WIDC_LISTBOX);
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT);
    this.#pic = this.getDlgItem<Static>(WIDC_STATIC1);
    this.#dropRect = this.getWndCtrl(WIDC_CUSTOM)?.rect ?? null;
    // the picture and the boss panel go under the controls of the layout (the picture box draws its frame and title over them)
    const client = this.getClientRect(false);
    this.#sprites = new SpriteLayer();
    this.addControl(this.#sprites, 0x7a11, client);
    this.controlList.splice(this.controlList.indexOf(this.#sprites), 1);
    this.controlList.unshift(this.#sprites);
    this.#texts = new TextLayer();
    this.addControl(this.#texts, 0x7a12, client);
    this.controlList.splice(this.controlList.indexOf(this.#texts), 1);
    this.controlList.splice(1, 0, this.#texts);
    this.publish();
    // `for each TPData row with a name: AddString`, `SetCurSel( 0 )`, `UpdateIndex()`
    this.#entries = this.#deps.ports.view.get().entries;
    this.#list?.resetContent();
    for (const e of this.#entries) this.#list?.addString(e.name);
    if (this.#entries.length > 0) {
      this.#list?.setCurSel(0);
      this.#select(0);
    }
    this.moveParentCenter();
  }

  // ---- the selection ---------------------------------------------------------------------------------------------------

  /** `UpdateIndex`: description, picture, the boss panel and the width. */
  #select(pos: number): void {
    const e = this.#entries[pos];
    if (!e) return;
    this.#sel = pos;
    this.#dropScroll = 0;
    this.#dropHover = -1;
    this.#text?.setLiteral(e.desc);
    this.#pic?.setTitle(e.name);
    const w = this.#defaultW + (e.boss ? TELE16_BOSS_WIDTH : 0);
    if (rectWidth(this.rectWindowValue) !== w) this.setWndSize(w, rectHeight(this.rectWindowValue));
    this.#paint();
  }

  /** The pictures and the panel of the selected entry. */
  #paint(): void {
    const e = this.#entries[this.#sel];
    const sprites: SpriteEntry[] = [];
    const texts: TextItem[] = [];
    if (e) {
      const box = this.getWndCtrl(WIDC_STATIC1)?.rect;
      if (box) {
        sprites.push({
          key: 'picture',
          path: `theme/${e.image !== '' ? e.image : DEFAULT_PICTURE}`,
          x: box.l,
          y: box.t + PIC_DROP,
          w: 240,
          h: 180,
        });
      }
      if (e.boss) this.#paintBoss(e.boss, sprites, texts);
    }
    this.#sprites.setEntries(sprites);
    this.#texts.setItems(texts);
  }

  #paintBoss(b: Tele16Boss, sprites: SpriteEntry[], texts: TextItem[]): void {
    const x = this.#defaultClientW - 10;
    const lines = [b.name, b.levelText, b.healthText, b.elementText, b.reqText, b.cooldownText];
    lines.forEach((text, i) => {
      texts.push({ x, y: PANEL_ROWS[i] ?? 0, text, color: BLACK });
    });
    if (b.element > 0 && this.#deps.elementPath) {
      const cut = this.#deps.elementCut?.(b.element - 1) ?? { x: (b.element - 1) * 16, y: 0 };
      // `m_texAttrIcon.Render( ( clientW + 55, 87 ), element - 1, 255, 1.2f, 1.2f )`
      sprites.push({
        key: 'element',
        path: this.#deps.elementPath,
        x: this.#defaultClientW + 55,
        y: 87,
        w: Math.round(16 * 1.2),
        h: Math.round(16 * 1.2),
        src: cut,
        scale: 1.2,
      });
    }
    // the drops: a grid of 30 px cells in `WIDC_CUSTOM`, `rowItems = width / 30`, scrolled by whole rows
    const r = this.#dropRect;
    if (r) {
      const cols = this.dropColumns();
      b.drops.forEach((d, i) => {
        const row = Math.floor(i / cols) - this.#dropScroll;
        if (row < 0 || (row + 1) * DROP_CELL > rectHeight(r) + DROP_CELL - 1) return;
        const hot = this.#dropHover === i;
        const size = hot ? Math.round(DROP_ICON * 1.1) : DROP_ICON;
        sprites.push({
          key: `drop:${String(i)}`,
          path: d.icon,
          x: r.l + (i % cols) * DROP_CELL,
          y: r.t + row * DROP_CELL,
          w: size,
          h: size,
        });
      });
    }
  }

  /** The number of icons per row of the drop grid. */
  dropColumns(): number {
    const r = this.#dropRect;
    return r ? Math.max(1, Math.floor(rectWidth(r) / DROP_CELL)) : 1;
  }

  /** The rows the drop grid can scroll (`SetScrollRange( 0, drops / rowItems )`, a page is `height / 30` rows). */
  dropMaxScroll(): number {
    const boss = this.#entries[this.#sel]?.boss;
    const r = this.#dropRect;
    if (!boss || !r) return 0;
    const rows = Math.ceil(boss.drops.length / this.dropColumns());
    return Math.max(0, rows - Math.floor(rectHeight(r) / DROP_CELL));
  }

  /** The drop index under a client point, `-1` for none. */
  dropAt(pt: Point): number {
    const r = this.#dropRect;
    const boss = this.#entries[this.#sel]?.boss;
    if (!r || !boss || !rectContains(r, pt)) return -1;
    const cols = this.dropColumns();
    const col = Math.floor((pt.x - r.l) / DROP_CELL);
    const row = Math.floor((pt.y - r.t) / DROP_CELL) + this.#dropScroll;
    if (col >= cols) return -1;
    const i = row * cols + col;
    return i >= 0 && i < boss.drops.length ? i : -1;
  }

  // ---- the pointer: drop tooltips and scrolling -----------------------------------------------------------------------------

  override onMouseMove(pt: Point): void {
    const i = this.dropAt(pt);
    if (i !== this.#dropHover) {
      this.#dropHover = i;
      this.#paint();
    }
    const boss = this.#entries[this.#sel]?.boss;
    const r = this.#dropRect;
    if (i < 0 || !boss || !r) return;
    const drop = boss.drops[i];
    if (!drop) return;
    const cols = this.dropColumns();
    const cell: Rect = {
      l: r.l + (i % cols) * DROP_CELL,
      t: r.t + (Math.floor(i / cols) - this.#dropScroll) * DROP_CELL,
      r: r.l + (i % cols) * DROP_CELL + DROP_CELL,
      b: r.t + (Math.floor(i / cols) - this.#dropScroll) * DROP_CELL + DROP_CELL,
    };
    const o = this.getScreenClientRect();
    this.#deps.tooltip.putItem(this, drop.view, rectOffset(cell, o.l, o.t), APP_TELE16);
  }

  override onMouseLeave(): void {
    if (this.#dropHover !== -1) {
      this.#dropHover = -1;
      this.#paint();
    }
  }

  override onMouseWheel(notches: number, pt: Point): void {
    const r = this.#dropRect;
    if (!r || !rectContains(r, pt) || this.dropMaxScroll() === 0) return;
    const next = Math.max(0, Math.min(this.dropMaxScroll(), this.#dropScroll - Math.sign(notches)));
    if (next !== this.#dropScroll) {
      this.#dropScroll = next;
      this.#paint();
    }
  }

  // ---- the buttons ---------------------------------------------------------------------------------------------------------

  /** The Teleport button: `SendTeleport( m_nCursorIndex )` and `Destroy()`. */
  teleport(): void {
    const index = this.selectedIndex;
    if (index < 0) return;
    this.#deps.ports.actions.teleport(index);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_LISTBOX && msg === Wnm.SELCHANGE) {
      const sel = this.#list?.getCurSel() ?? -1;
      if (sel >= 0 && sel < this.#entries.length) this.#select(sel);
      return true;
    }
    if (id === WIDC_BUTTON && msg === Wnm.CLICKED) {
      this.teleport();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
