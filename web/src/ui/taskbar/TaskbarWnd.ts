// `TaskbarWnd` (`CWndTaskBar`, WndTaskBar.cpp:1174-1460; Plan/42 §11; WP-UI-14): the 48 px `WBS_MANAGER` frame at the
// bottom of the virtual viewport. The layout (`APP_TASKBAR<w>`) is chosen by the virtual width, its bitmap is painted at
// its native size from the left (the slots sit at fixed layout coordinates; only the right end follows the width), the
// start button and the page arrows keep the layout coordinates (left-aligned) and the three slot rows are painted on the
// custom canvases `WIDC_CUSTOM1/2/3` from the local `TaskbarModel`.
//
// Execution of non-applet shortcuts, drag and drop (WP-UI-19), the server messages (WP-UI-18) and the action queue
// rules (WP-UI-42) sit behind the ports of `TaskbarPorts`.

import { APP_TRADE, WIDC_BUTTON1, WIDC_DOWN, WIDC_UP } from '../../gen/defs/resdata';
import { Button } from '../controls/Button';
import { SC, type Shortcut } from '../dnd/shortcut';
import type { Point, Rect } from '../layout/geometry';
import type { WidcKey } from '../layout/registry';
import { rectXYWH } from '../layout/geometry';
import { WBS_MANAGER, WBS_MENUITEM, WBS_MOVE, Wnm } from '../layout/styles';
import type { BakeSpec } from '../skin/baker';
import { MISSING, themePath, type UiImageCache } from '../skin/images';
import { FONT, cssFont } from '../text/fonts';
import type { TooltipService } from '../tooltip/TooltipService';
import { appletTooltipText } from '../tooltip/appletTooltip';
import type { AppletRegistry } from '../wm/applets';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import {
  ACTION_ICON_DX,
  MAX_SLOT_ITEM,
  MAX_SLOT_QUEUE,
  QUEUE_SLOT_H,
  QUEUE_SLOT_W,
  SLOT_SIZE,
  type SlotHit,
  TASKBAR_H,
  type TaskbarLayoutSpec,
  hitSlot,
} from './layout';
import { type TaskbarModel } from './model';
import { type DropDecision, decideDrop, pickupShortcut, type DropRow } from './drop';
import {
  drawImageScaled,
  drawRadar,
  paintTaskbarBackground,
  taskbarBackgroundSlices,
} from './paint';
import { actionPointShade, lampSlots, queueApCost } from './queueView';
import type { SlotIcon, TaskbarPorts } from './ports';

export type { SlotIcon, TaskbarActions, TaskbarPorts } from './ports';

export interface TaskbarWndDeps {
  readonly spec: TaskbarLayoutSpec;
  readonly model: TaskbarModel;
  readonly ports: TaskbarPorts;
  readonly applets: AppletRegistry;
  readonly images: UiImageCache;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  /** The virtual width and the bottom of the layout rect when the taskbar is (re)placed. */
  readonly viewport: () => { width: number; layoutBottom: number };
  /** `wm.objectExecutorApplet`. */
  readonly executeApplet: (app: number) => void;
  /** Opens the start menu (§11.4). */
  readonly openStartMenu: () => void;
}

const SLOT_BG = 'rgba(0, 0, 0, 0.0)';
/** `thick` of `RenderOutLineLamp`. */
const LAMP_THICK = 10;
/** The cool-down radars and the lamp repaint about ten times a second. */
const ANIM_MS = 100;

export class TaskbarWnd extends WndNeuz {
  readonly appId: number;
  readonly spec: TaskbarLayoutSpec;
  readonly #deps: TaskbarWndDeps;
  #pressed: SlotHit | null = null;
  #unsub: (() => void) | null = null;
  #offDrag: (() => void) | null = null;
  #offClaim: (() => void) | null = null;
  #highlight: HTMLCanvasElement | null = null;
  /** The outline lamp of the action queue (`RenderOutLineLamp`): a glow canvas pulsing by CSS, redrawn when the number of lit slots changes. */
  #lamp: HTMLCanvasElement | null = null;
  #lampShown = -1;
  #lastAnim = Number.NEGATIVE_INFINITY;
  /** The icon paths of the item slots shown at the last check, joined (an empty bar is nine separators). */
  #iconSig = '|'.repeat(MAX_SLOT_ITEM);
  /** Origins of the three rows in client coordinates (the layout's placeholders). */
  #origins = { applets: { x: 0, y: 0 }, items: { x: 0, y: 0 }, queue: { x: 0, y: 0 } };
  readonly #loading = new Set<string>();

  constructor(deps: TaskbarWndDeps) {
    super();
    this.#deps = deps;
    this.spec = deps.spec;
    this.appId = deps.spec.app;
    // the taskbar is part of the root layer: no regInfo, no caption
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MANAGER | extraStyle);
  }

  override onInitialUpdate(): void {
    // not movable (§11.1): the layout style carries WBS_MOVE
    this.delWndStyle(WBS_MOVE);
    this.#place();
    super.onInitialUpdate();
    for (const [key, slot] of [
      ['WIDC_CUSTOM1', 'applets'],
      ['WIDC_CUSTOM2', 'items'],
      ['WIDC_CUSTOM3', 'queue'],
    ] as const) {
      const c = this.getWndCtrl(key);
      if (c) this.#origins[slot] = { x: c.rect.l, y: c.rect.t };
    }
    // the action-skill icon sits left of the queue (§11.1)
    const q = this.#origins.queue;
    this.addCustomCanvas('action', rectXYWH(q.x - ACTION_ICON_DX, q.y, SLOT_SIZE, SLOT_SIZE));
    for (const key of ['WIDC_CUSTOM1', 'WIDC_CUSTOM2', 'WIDC_CUSTOM3'] as const) {
      this.customCanvas(key);
    }
    this.#unsub = this.#deps.model.state.subscribe(() => {
      this.#paintLamp();
      this.invalidate();
    });
    // the drag highlight (§6.3, `m_dwHighAlpha`): a blue bar under the rows that take the dragged shortcut, pulsing by CSS
    const r = this.rectWindowValue;
    this.#highlight = this.addCustomCanvas('highlight', rectXYWH(0, 0, r.r - r.l, r.b - r.t));
    this.#offDrag = this.env.wm.drag.subscribe(() => {
      this.#paintHighlight();
    });
    // the lamp around the lit queue slots: 10 px of glow around the first `show` cells (the part above the bar is clipped)
    this.#lamp = this.addCustomCanvas(
      'lamp',
      rectXYWH(
        q.x - LAMP_THICK,
        q.y + 1 - LAMP_THICK,
        MAX_SLOT_QUEUE * QUEUE_SLOT_W + 2 * LAMP_THICK,
        QUEUE_SLOT_H + 2 * LAMP_THICK,
      ),
    );
    if (this.#lamp) {
      this.#lamp.style.pointerEvents = 'none';
      this.#lamp.style.visibility = 'hidden';
    }
    this.#paintLamp();
    // a slot dragged out of the bar onto the world deletes the slot (§11.3); no forbid icon, the bar took it
    this.#offClaim = this.env.wm.drag.addWorldClaimer((sc) => {
      if (sc.from !== this) return false;
      this.#removeSource(sc);
      return true;
    });
    this.invalidate();
  }

  /** The frame rect: the full virtual width at the bottom of the layout rect. */
  #place(): void {
    const { width, layoutBottom } = this.#deps.viewport();
    this.setWndRect({ l: 0, t: layoutBottom, r: width, b: layoutBottom + TASKBAR_H });
  }

  /** Called when the virtual viewport changed: the frame follows (the layout choice is the service's). */
  replace(): void {
    this.#place();
    this.requestBake();
  }

  override onDestroy(): void {
    this.#unsub?.();
    this.#unsub = null;
    this.#offDrag?.();
    this.#offDrag = null;
    this.#offClaim?.();
    this.#offClaim = null;
    super.onDestroy();
  }

  // ---- skin: the layout bitmap at its native size, the right end follows the width ---------------------------------

  override buildBakeSpec(): BakeSpec | null {
    const r = this.rectWindowValue;
    const w = r.r - r.l;
    const h = r.b - r.t;
    if (w <= 0 || h <= 0 || this.texture === '') return null;
    const path = themePath(this.texture);
    return {
      width: w,
      height: h,
      ops: [],
      afterImages: [{ path, mode: 'nibble' }],
      after: (ctx) => {
        const img = ctx.image(path, 'nibble');
        if (img && img !== MISSING) {
          paintTaskbarBackground(ctx.buffer, img, taskbarBackgroundSlices(this.spec.legacyWidth));
        }
      },
    };
  }

  // ---- painting of the slot rows -----------------------------------------------------------------------------

  override onDraw(id: WidcKey, ctx: CanvasRenderingContext2D): void {
    const s = this.#deps.model.get();
    if (id === 'WIDC_CUSTOM1') {
      for (let i = 0; i < this.spec.appletSlots; i += 1) {
        this.#paintSlot(ctx, s.applets[i] ?? null, i * SLOT_SIZE, 0, SLOT_SIZE, SLOT_SIZE);
      }
    } else if (id === 'WIDC_CUSTOM2') {
      const page = s.items[s.page] ?? [];
      for (let i = 0; i < MAX_SLOT_ITEM; i += 1) {
        this.#paintSlot(ctx, page[i] ?? null, i * SLOT_SIZE, 0, SLOT_SIZE, SLOT_SIZE);
      }
    } else if (id === 'WIDC_CUSTOM3') {
      for (let i = 0; i < MAX_SLOT_QUEUE; i += 1) {
        this.#paintSlot(
          ctx,
          s.queue[i] ?? null,
          i * QUEUE_SLOT_W,
          0,
          QUEUE_SLOT_W,
          QUEUE_SLOT_H,
          false,
        );
      }
      this.#paintQueueState(ctx);
    } else if ((id as string) === 'action') {
      this.#paintPath(ctx, 'icon/icon_actionskill.dds', 0, 0, SLOT_SIZE, SLOT_SIZE);
    }
  }

  #paintSlot(
    ctx: CanvasRenderingContext2D,
    sc: Shortcut | null,
    x: number,
    y: number,
    w: number,
    h: number,
    radar = true,
  ): void {
    ctx.fillStyle = SLOT_BG;
    ctx.fillRect(x, y, w, h);
    if (!sc || sc.kind === SC.NONE) return;
    const icon = this.#iconOf(sc);
    if (!icon) return;
    this.#paintPath(ctx, icon.path, x, y, w, h);
    if (icon.count !== undefined && icon.count > 1) {
      this.#paintCount(ctx, icon.count, x + w - 1, y + h - 2);
    }
    // the cool-down of an item group / a skill (`RenderRadar`): not on the queue row (the original draws none there)
    if (radar && icon.cooldown) {
      drawRadar(
        ctx,
        x,
        y,
        this.env.host.now() - icon.cooldown.startMs,
        icon.cooldown.endMs - icon.cooldown.startMs,
      );
    }
  }

  /**
   * The state of the action queue over its cells (`CWndTaskBar::OnDraw`): the cell being executed is outlined, and unless
   * `SM_ACTPOINT` is on the cells from the second on are shaded for the action points that are still missing (10 / 25 / 50 / 100 points
   * are 1 / 2 / 3 / 4 cells).
   */
  #paintQueueState(ctx: CanvasRenderingContext2D): void {
    const s = this.#deps.model.get();
    const free = this.#deps.ports.actionPointsFree?.() ?? false;
    const count = s.queue.filter((e) => e !== null).length;
    if (!free) {
      const shade = actionPointShade(s.actionPoints, QUEUE_SLOT_W);
      ctx.fillStyle = 'rgba(0, 0, 0, 0.498)';
      ctx.fillRect(QUEUE_SLOT_W + shade.lit, 0, shade.dark, QUEUE_SLOT_H + 2);
    }
    if (s.executing !== 0 && count > 0 && s.queueIndex < count) {
      ctx.strokeStyle = 'rgba(255, 224, 64, 0.95)';
      ctx.lineWidth = 2;
      ctx.strokeRect(s.queueIndex * QUEUE_SLOT_W + 1, 1, QUEUE_SLOT_W - 2, QUEUE_SLOT_H - 2);
    }
  }

  /** The lamp canvas: shown while the queue is not empty and costs points (`show` cells lit by the action points). */
  #paintLamp(): void {
    const canvas = this.#lamp;
    if (!canvas) return;
    const s = this.#deps.model.get();
    const free = this.#deps.ports.actionPointsFree?.() ?? false;
    const count = s.queue.filter((e) => e !== null).length;
    const show = free ? 0 : lampSlots(s.actionPoints, count);
    if (show === this.#lampShown) return;
    this.#lampShown = show;
    canvas.style.visibility = show > 0 ? 'visible' : 'hidden';
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (show <= 0) {
      canvas.classList.remove('flyff-drag-pulse');
      return;
    }
    // `dwColor2` pulses from alpha 196 to 0 and back every 40 frames (`fLerp += 0.05`): a fixed gradient whose opacity alternates
    canvas.classList.add('flyff-drag-pulse');
    canvas.style.animationDirection = 'alternate-reverse';
    canvas.style.animationDuration = `${String(20 * (1000 / 60))}ms`;
    const w = show * QUEUE_SLOT_W;
    const h = QUEUE_SLOT_H;
    const edge = 'rgba(255, 255, 255, 0)';
    const glow = 'rgba(40, 0, 255, 0.769)';
    const bar = (
      x0: number,
      y0: number,
      x1: number,
      y1: number,
      gx0: number,
      gy0: number,
      gx1: number,
      gy1: number,
    ): void => {
      const g = ctx.createLinearGradient(gx0, gy0, gx1, gy1);
      g.addColorStop(0, edge);
      g.addColorStop(1, glow);
      ctx.fillStyle = g;
      ctx.fillRect(x0, y0, x1 - x0, y1 - y0);
    };
    const o = LAMP_THICK;
    // left, top, right, bottom (the four rects of the original; the corners stay empty)
    bar(0, o, o, o + h, 0, 0, o, 0);
    bar(o, 0, o + w, o, 0, 0, 0, o);
    bar(o + w, o, o + w + o, o + h, o + w + o, 0, o + w, 0);
    bar(o, o + h, o + w, o + h + o, 0, o + h + o, 0, o + h);
  }

  /** The icon of a slot: applets from the registry, every other kind through the port. */
  #iconOf(sc: Shortcut): SlotIcon | null {
    if (sc.kind === SC.APPLET) {
      const e = this.#deps.applets.get(sc.id);
      return e?.icon ? { path: `icon/${e.icon}` } : null;
    }
    return this.#deps.ports.slotIcon(sc);
  }

  #paintPath(
    ctx: CanvasRenderingContext2D,
    path: string,
    x: number,
    y: number,
    w: number,
    h: number,
  ): void {
    const img = this.#deps.images.peek(path, 'exact');
    if (img !== undefined && img !== MISSING) {
      drawImageScaled(ctx, img, x, y, w, h);
      return;
    }
    if (img === undefined && !this.#loading.has(path)) {
      this.#loading.add(path);
      void this.#deps.images.get(path, 'exact').then(() => {
        this.#loading.delete(path);
        if (!this.destroyed) this.invalidate();
      });
    }
  }

  /** The stack count: `#EAEBDD` outlined with `features.new_font_color`, the legacy blue pair without it (§8.10). */
  #paintCount(ctx: CanvasRenderingContext2D, n: number, right: number, bottom: number): void {
    ctx.font = cssFont(FONT.text);
    ctx.textAlign = 'right';
    ctx.textBaseline = 'alphabetic';
    const t = String(n);
    const newColor = this.#deps.ports.newFontColor?.() ?? true;
    if (newColor) {
      ctx.fillStyle = '#000000';
      for (const [dx, dy] of [
        [-1, 0],
        [1, 0],
        [0, -1],
        [0, 1],
      ] as const) {
        ctx.fillText(t, right + dx, bottom + dy);
      }
      ctx.fillStyle = '#EAEBDD';
      ctx.fillText(t, right, bottom);
    } else {
      ctx.fillStyle = '#1d3b7a';
      ctx.fillText(t, right + 1, bottom + 1);
      ctx.fillStyle = '#6aa0ff';
      ctx.fillText(t, right, bottom);
    }
  }

  // ---- interaction (§11.3) -----------------------------------------------------------------------------------

  /** The slot under a point of the taskbar's client rect. */
  slotAt(pt: Point): SlotHit | null {
    return hitSlot(pt, this.#origins, this.spec.appletSlots);
  }

  /** The screen rect of a slot (tooltips, drops). */
  slotScreenRect(hit: SlotHit): Rect {
    const o =
      hit.row === 'applet'
        ? this.#origins.applets
        : hit.row === 'item'
          ? this.#origins.items
          : this.#origins.queue;
    const w = hit.row === 'queue' ? QUEUE_SLOT_W : SLOT_SIZE;
    const h = hit.row === 'queue' ? QUEUE_SLOT_H : SLOT_SIZE;
    const s = this.getScreenClientRect();
    return rectXYWH(s.l + o.x + hit.slot * w, s.t + o.y, w, h);
  }

  /** The shortcut of a slot row on the page being shown (`null` = empty). */
  slotShortcutOf(hit: SlotHit): Shortcut | null {
    const sc = this.#deps.model.slot(hit.row, hit.slot);
    return sc && sc.kind !== SC.NONE ? sc : null;
  }

  override onLButtonDown(pt: Point): void {
    this.#pressed = this.slotAt(pt);
    const hit = this.#pressed;
    if (!hit) return;
    const sc = this.slotShortcutOf(hit);
    if (!sc) return;
    // a drag starts at the first pointer move with the button down (`OnMouseMove`: `m_pSelectShortcut`)
    this.env.wm.drag.arm(this, () => this.#dragPayload(hit));
  }

  /** The `m_GlobalShortcut` of a drag from a slot: a copy of the slot with the icon and the slot it came from. */
  #dragPayload(hit: SlotHit): Shortcut | null {
    const sc = this.slotShortcutOf(hit);
    if (!sc) return null;
    const page = this.#deps.model.get().page;
    const icon = this.#deps.ports.iconOf?.(sc) ?? this.#iconOf(sc)?.path;
    return {
      ...pickupShortcut(sc),
      from: this,
      ...(icon ? { icon } : {}),
      srcSlot: { row: hit.row, ...(hit.row === 'item' ? { page } : {}), slot: hit.slot },
    };
  }

  /** Down and up on the same non-empty slot executes it; queue slots do not execute. */
  override onLButtonUp(pt: Point): void {
    const hit = this.slotAt(pt);
    const pressed = this.#pressed;
    this.#pressed = null;
    if (!hit || !pressed) return;
    if (hit.row !== pressed.row || hit.slot !== pressed.slot) return;
    if (hit.row === 'queue') return;
    const sc = this.slotShortcutOf(hit);
    if (!sc) return;
    this.executeShortcut(sc);
  }

  override onPointerRelease(): void {
    this.#pressed = null;
  }

  /**
   * `ObjectExecutor(SHORTCUT)` of one slot (§11.5): applets toggle through the manager (`APP_TRADE` requests a trade with
   * the focus mover instead of opening a window), the rest goes through the port.
   */
  executeShortcut(sc: Shortcut): void {
    if (sc.kind === SC.APPLET) {
      if (sc.id === APP_TRADE && this.#deps.ports.tradeRequest) {
        this.#deps.ports.tradeRequest();
        return;
      }
      this.#deps.executeApplet(sc.id);
      return;
    }
    this.#deps.ports.execute(sc);
  }

  /** Right-click up on a slot removes it: applet -> `removeApplet`, item -> `removeItem`, queue -> the queue port. */
  override onRButtonUp(pt: Point): void {
    const hit = this.slotAt(pt);
    if (!hit) return;
    const { model, ports } = this.#deps;
    const sc = model.slot(hit.row, hit.slot);
    if (!sc || sc.kind === SC.NONE) return;
    if (hit.row === 'applet') {
      model.setApplet(hit.slot, null);
      ports.actions.removeApplet(hit.slot);
    } else if (hit.row === 'item') {
      const page = model.get().page;
      model.setItem(page, hit.slot, null);
      ports.actions.removeItem(page, hit.slot);
    } else if (ports.queue) {
      ports.queue.remove(hit.slot);
    } else {
      // §11.8 `removeQueue(index)`: shifts left; the queue rules (checkAddSkill) are 45's; the plain shift is here
      const q = model.get().queue.filter((e, i) => e !== null && i !== hit.slot);
      model.setQueue(q);
      ports.actions.sendQueue(model.get().queue);
    }
  }

  /** Hover over a slot: the applet tooltip (`description` + `[Hotkey X]`), or the port's text for the other kinds. */
  override onMouseMove(pt: Point): void {
    const hit = this.slotAt(pt);
    if (!hit) return;
    const sc = this.#deps.model.slot(hit.row, hit.slot);
    if (hit.row === 'queue') {
      // the action points and what the cell costs (45 §19.3); an empty cell shows the points alone
      const text = this.#deps.ports.queueTooltip?.(
        this.#deps.model.get().actionPoints,
        hit.slot,
        queueApCost(hit.slot),
      );
      if (text) this.#deps.tooltip.put(this, text, this.slotScreenRect(hit), 0);
      if (!sc || sc.kind === SC.NONE || text) return;
    }
    if (!sc || sc.kind === SC.NONE) return;
    const rect = this.slotScreenRect(hit);
    // item and skill tooltips come from the tooltip host with the composers of 45
    if (this.#deps.ports.slotTooltipRich?.(sc, rect) === true) return;
    let text: string | null = null;
    if (sc.kind === SC.APPLET) {
      const e = this.#deps.applets.get(sc.id);
      if (e) {
        const desc =
          e.tooltipTid !== null ? this.#deps.getText(e.tooltipTid) : (e.tooltipText ?? '');
        text = appletTooltipText(desc, this.#deps.applets.hotkeyOf(sc.id), this.#deps.getText);
      }
    } else if (sc.kind === SC.CHAT) {
      text = sc.text ?? null;
    } else {
      text = this.#deps.ports.slotTooltip?.(sc) ?? null;
    }
    if (text) this.#deps.tooltip.put(this, text, rect, 0);
  }

  // ---- drops (`OnDropIcon`, §11.6) -----------------------------------------------------------------------------------

  /** The CHAT shortcuts of the item rows and the applet row (the client count of the 10-macro limit). */
  #chatCount(): number {
    const s = this.#deps.model.get();
    const chat = (sc: Shortcut | null): boolean => sc?.kind === SC.CHAT;
    return s.applets.filter(chat).length + s.items.reduce((n, p) => n + p.filter(chat).length, 0);
  }

  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    const o = {
      x: this.rectClientValue.l - this.rectWindowValue.l,
      y: this.rectClientValue.t - this.rectWindowValue.t,
    };
    const hit = this.slotAt({ x: ptWindow.x - o.x, y: ptWindow.y - o.y });
    const { ports, model } = this.#deps;
    const from = sc.from;
    const fromFrame = from?.getFrameWnd() ?? null;
    const d = decideDrop(sc, hit, {
      fromApp: fromFrame ? fromFrame.appId : null,
      fromSelf: from === this,
      fromMenu: from instanceof Button && from.isWndStyle(WBS_MENUITEM),
      skill: (type, id) => ports.skills?.info(type, id) ?? null,
      items: ports.items ?? null,
      chatCount: this.#chatCount(),
      page: model.get().page,
    });
    return this.#applyDrop(d, sc);
  }

  #applyDrop(d: DropDecision, sc: Shortcut): boolean {
    const { ports, model } = this.#deps;
    const drag = this.env.wm.drag;
    switch (d.kind) {
      case 'forbid':
        drag.forbid();
        return false;
      case 'ignore':
        return false;
      case 'message':
        ports.message?.(d.tid);
        return false;
      case 'removeSource':
        this.#removeSource(sc);
        return true;
      case 'queue': {
        const q = ports.queue;
        // a move inside the queue removes the old entry first, without a message (the add sends the whole queue)
        const fromQueue = sc.from === this && sc.srcSlot?.row === 'queue';
        if (fromQueue && sc.srcSlot) q?.remove(sc.srcSlot.slot, false);
        const icon = ports.iconOf?.(sc) ?? sc.icon ?? null;
        if (!q?.add(d.slot, sc.type, sc.id, icon)) {
          drag.forbid();
          return false;
        }
        if (sc.from === this && !fromQueue) this.#removeSource(sc);
        return true;
      }
      case 'set': {
        const sh = d.shortcut;
        if (d.row === 'applet') {
          model.setApplet(d.slot, sh);
          ports.actions.addApplet(d.slot, sh);
        } else {
          const page = model.get().page;
          model.setItem(page, d.slot, sh);
          ports.actions.addItem(page, d.slot, sh);
        }
        // a moved slot: the original is deleted after the copy was sent (§11.6)
        if (sc.from === this) this.#removeSource(sc);
        return true;
      }
    }
  }

  /** Deletes the slot a dragged shortcut came from: model first, then the matching message. */
  #removeSource(sc: Shortcut): void {
    const src = sc.srcSlot;
    if (!src) return;
    const { ports, model } = this.#deps;
    if (src.row === 'applet') {
      model.setApplet(src.slot, null);
      ports.actions.removeApplet(src.slot);
    } else if (src.row === 'item') {
      const page = src.page ?? model.get().page;
      model.setItem(page, src.slot, null);
      ports.actions.removeItem(page, src.slot);
    } else {
      ports.queue?.remove(src.slot);
    }
  }

  // ---- the item sync (`UpdateItem` + the applet row of `OnDraw`) ----------------------------------------------

  override process(now: number): void {
    this.#syncItems();
    this.#syncIcons();
    // a cool-down runs (the radar) or the points changed by the clock: repaint about ten times a second
    if (now - this.#lastAnim >= ANIM_MS && this.#deps.ports.animating?.() === true) {
      this.#lastAnim = now;
      this.invalidate();
    }
    this.#paintLamp();
    super.process(now);
  }

  /**
   * A shown item slot whose PICTURE changed without a change of the model repaints the bar: a pet that grew a grade
   * (`item/itm_tiger_00.dds` -> `_01`, bug #017) keeps its slot, its id and its shortcut, only the icon path the port answers differs.
   */
  #syncIcons(): void {
    const s = this.#deps.model.get();
    const row = s.items[s.page] ?? [];
    let sig = '';
    for (let i = 0; i < MAX_SLOT_ITEM; i += 1) {
      const sc = row[i];
      sig += `${sc?.kind === SC.ITEM ? (this.#iconOf(sc)?.path ?? '') : ''}|`;
    }
    if (sig === this.#iconSig) return;
    this.#iconSig = sig;
    this.invalidate();
  }

  /**
   * An ITEM shortcut whose object id no longer exists follows its stack when the item is stackable and another stack
   * exists (the id is replaced and `SendAdd*TaskBar` repeats the slot), else the slot is emptied and `SendRemove*` goes
   * out (source: `UpdateItem` WndTaskBar.cpp:1118-1169; the plan's "emptied locally only" does not match the C++).
   */
  #syncItems(): void {
    const items = this.#deps.ports.items;
    if (!items || items.ready?.() === false) return;
    const { model, ports } = this.#deps;
    const s = model.get();
    const check = (sc: Shortcut | null, row: DropRow, slot: number, page: number): void => {
      if (sc?.kind !== SC.ITEM || items.has(sc.id)) return;
      const itemId = sc.itemId;
      const other = itemId !== undefined && items.stackable(itemId) ? items.find(itemId) : null;
      if (other !== null) {
        const next = { ...sc, id: other };
        if (row === 'applet') {
          model.setApplet(slot, next);
          ports.actions.addApplet(slot, next);
        } else {
          model.setItem(page, slot, next);
          ports.actions.addItem(page, slot, next);
        }
      } else if (row === 'applet') {
        model.setApplet(slot, null);
        ports.actions.removeApplet(slot);
      } else {
        model.setItem(page, slot, null);
        ports.actions.removeItem(page, slot);
      }
    };
    for (let i = 0; i < this.spec.appletSlots; i += 1) check(s.applets[i] ?? null, 'applet', i, 0);
    const row = s.items[s.page] ?? [];
    for (let i = 0; i < MAX_SLOT_ITEM; i += 1) check(row[i] ?? null, 'item', i, s.page);
  }

  // ---- the drag highlight ---------------------------------------------------------------------------------------

  /** Draws the blue bars of the rows that take the dragged shortcut (5 px high at y = 42, `OnDraw` of the original). */
  #paintHighlight(): void {
    const canvas = this.#highlight;
    if (!canvas) return;
    const sc = this.env.wm.draggedShortcut;
    const ctx = canvas.getContext('2d');
    canvas.style.visibility = sc ? 'visible' : 'hidden';
    if (!sc) {
      canvas.classList.remove('flyff-drag-pulse');
      return;
    }
    // the pulse is `m_dwHighAlpha += 10` per frame: up and down over 26 frames (CSS, alternating)
    canvas.classList.add('flyff-drag-pulse');
    canvas.style.animationDirection = 'alternate';
    canvas.style.animationDuration = `${String(13 * (1000 / 60))}ms`;
    canvas.style.pointerEvents = 'none';
    if (!ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = 'rgba(0, 0, 255, 0.5)';
    const o = this.#origins;
    const widths = {
      applet: this.spec.appletSlots * SLOT_SIZE,
      item: MAX_SLOT_ITEM * SLOT_SIZE,
      queue: MAX_SLOT_QUEUE * QUEUE_SLOT_W,
    };
    const bar = (row: DropRow): void => {
      const origin = row === 'applet' ? o.applets : row === 'item' ? o.items : o.queue;
      const pad = row === 'queue' ? [3, 4] : [6, 6];
      ctx.fillRect(origin.x - (pad[0] ?? 0), 42, widths[row] + (pad[0] ?? 0) + (pad[1] ?? 0), 5);
    };
    if (sc.kind === SC.APPLET || sc.kind === SC.ITEM || sc.kind === SC.MOTION) {
      bar('applet');
      bar('item');
    } else if (sc.kind === SC.SKILL) {
      if (sc.type === 0) {
        const info = this.#deps.ports.skills?.info(0, sc.id);
        if (info && (info.combo === 1 || info.combo === 4)) bar('item');
        bar('queue');
      } else {
        bar('item');
      }
    }
  }

  // ---- buttons -----------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.#deps.openStartMenu();
        return true;
      }
      if (id === WIDC_UP) {
        this.#deps.model.setPage(this.#deps.model.get().page - 1);
        return true;
      }
      if (id === WIDC_DOWN) {
        this.#deps.model.setPage(this.#deps.model.get().page + 1);
        return true;
      }
      // an item of the start menu (`CWndTaskBar::OnCommand`: `ObjectExecutor( SHORTCUT_APPLET, nID )`): the id of the item is the applet's
      if (id !== 0 && src instanceof Button && src.isWndStyle(WBS_MENUITEM)) {
        this.#deps.executeApplet(id);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** The start button (tests, the start menu's anchor). */
  get startButton(): Button | null {
    return this.getDlgItem<Button>('WIDC_BUTTON1');
  }
}
