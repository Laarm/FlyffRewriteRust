// `WndNavigator` (`CWndNavigator`, `src:_Interface/WndField.cpp:9546-10640`; ref/22 §17 C, ref/99 §4.4, Plan/42 WP-UI-36, 45 §7.6):
// `APP_NAVIGATOR`, the resizable minimap window. The shell hosts the canvas (`NavMapCtrl`), the three 16 x 16 buttons (the place menu
// `P` at the top-left, the zoom buttons at the bottom-left), the title (the region name) and the pointer input; the drawing and every
// decision behind a click belong to 45's `features/navigator` (the ports).
//
//   size      144 x 160 at the start, `AdjustMinRect( 16 * 6, 16 * 7 )`, `AdjustMaxRect( 16 * 12, 16 * 13 )`; a resize moves in steps of 16 px
//   position  the top-right corner of the layout rect (`Move( rectRoot.right - width, rectRoot.top )`)
//   buttons   100000 place (disabled indoors), 100005 zoom in at (0, cy - 32), 100006 zoom out at (0, cy - 16)
//   input     L-down: own mark + `SetNaviPointReq` + selection; L-up on an NPC focus clears it; R-down clears focus and destination and
//             opens the list of NPCs in sight; double click (game master): `/teleport`; a move over a dot or a mark: its name
//   Esc       the navigator is never closed by Esc (`ESC_SKIP_APPS`)

import { APP_NAVIGATOR } from '../../../gen/defs/resdata';
import {
  TID_APP_NAVIGATOR,
  TID_GAME_MONSTER,
  TID_GAME_NPC,
  TID_GAME_PARTYTEXT,
  TID_GAME_PLAYER,
} from '../../../gen/defs/text';
import { Button } from '../../controls/Button';
import { Menu } from '../../controls/Menu';
import {
  type FrameRects,
  type Point,
  type Rect,
  rectDeflate,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../../layout/geometry';
import { WBS_NODRAWFRAME, WBS_THICKFRAME, Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import type { TooltipService } from '../../tooltip/TooltipService';
import { adjustMaxRect, adjustMinRect } from '../../wm/dragMath';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { MoverMenu } from '../npc/MoverMenu';
import { NavMapCtrl } from './NavMapCtrl';
import {
  EMPTY_NAVIGATOR_VIEW,
  type NavFilterView,
  type NavigatorPorts,
  type NavigatorView,
  type NavSize,
} from './ports';

/** The control ids of the original (`m_wndPlace.Create(..., 100000)`, zoom in 100005, zoom out 100006). */
export const NAV_ID_PLACE = 100000;
export const NAV_ID_ZOOM_IN = 100005;
export const NAV_ID_ZOOM_OUT = 100006;
export const NAV_ID_MAP = 100010;

/** Default and limits of the window size (`SetWndRect`). */
export const NAV_MIN = { w: 16 * 6, h: 16 * 7 } as const;
export const NAV_MAX = { w: 16 * 12, h: 16 * 13 } as const;

/** The place menu: a `Menu` whose checks mirror the filters (`m_wndMenuPlace`, items 0..3 = `OBJFILTER_*`). */
class PlaceMenu extends Menu {}

export interface NavigatorDeps {
  readonly ports: NavigatorPorts;
  readonly getText: (tid: number) => string;
  readonly tooltip: Pick<TooltipService, 'put' | 'cancel'>;
}

export class WndNavigator extends WndNeuz {
  readonly appId = APP_NAVIGATOR;
  override readonly processEveryFrame = true;
  readonly #deps: NavigatorDeps;
  #map!: NavMapCtrl;
  #place: Button | null = null;
  #zoomIn: Button | null = null;
  #zoomOut: Button | null = null;
  #placeMenu: PlaceMenu | null = null;
  #moverMenu: MoverMenu | null = null;
  #applied: NavigatorView | null = null;
  #restored: { filters: NavFilterView; zoom: number } | null = null;

  constructor(deps: NavigatorDeps) {
    super();
    this.#deps = deps;
  }

  /** `Initialize`: `InitDialog(APP_NAVIGATOR)`; the layout style already carries `WBS_THICKFRAME`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_THICKFRAME | extraStyle);
  }

  // ---- geometry ---------------------------------------------------------------------------------------------------------

  /** `CWndNavigator::SetWndRect`: the minimum and the maximum size, then the normal frame rects. */
  protected override computeRects(window: Rect): FrameRects {
    let w = adjustMinRect(window, this.rectWindowValue, NAV_MIN.w, NAV_MIN.h);
    w = adjustMaxRect(w, this.rectWindowValue, NAV_MAX.w, NAV_MAX.h);
    return super.computeRects(w);
  }

  /** `AdjustMinRect` / `AdjustMaxRect` while the user drags an edge. */
  override constrainResize(next: Rect, current: Rect): Rect {
    return adjustMaxRect(
      adjustMinRect(next, current, NAV_MIN.w, NAV_MIN.h),
      current,
      NAV_MAX.w,
      NAV_MAX.h,
    );
  }

  /** The client size (what the map is drawn into). */
  get mapSize(): NavSize {
    const c = this.getClientRect(false);
    return { w: rectWidth(c), h: rectHeight(c) };
  }

  get map(): NavMapCtrl {
    return this.#map;
  }
  get placeMenu(): Menu | null {
    return this.#placeMenu;
  }
  get moverMenu(): MoverMenu | null {
    return this.#moverMenu;
  }
  get zoomInButton(): Button | null {
    return this.#zoomIn;
  }
  get zoomOutButton(): Button | null {
    return this.#zoomOut;
  }
  get placeButton(): Button | null {
    return this.#place;
  }

  // ---- OnInitialUpdate ---------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const c = this.getClientRect(false);
    // the minimap: under the buttons, over the whole client
    this.#map = this.addControl(new NavMapCtrl(), NAV_ID_MAP, c, WBS_NODRAWFRAME);
    this.#map.paint = (ctx, size, now) => {
      this.#deps.ports.paint(ctx, size, now);
    };
    const button = (id: number, texture: string, rect: Rect): Button => {
      const b = new Button();
      this.addControl(b, id, rect, WBS_NODRAWFRAME);
      b.setTexture(texture);
      return b;
    };
    this.#place = button(NAV_ID_PLACE, 'ButtNavLeft.tga', { l: 0, t: 1, r: 16, b: 17 });
    this.#zoomIn = button(NAV_ID_ZOOM_IN, 'ButtNavZoomIn.tga', {
      l: 0,
      t: rectHeight(c) - 32,
      r: 16,
      b: rectHeight(c) - 16,
    });
    this.#zoomOut = button(NAV_ID_ZOOM_OUT, 'ButtNavZoomOut.tga', {
      l: 0,
      t: rectHeight(c) - 16,
      r: 16,
      b: rectHeight(c),
    });
    // the place menu (the checks follow the filters) and the popup of the NPCs in sight
    const menu = new PlaceMenu();
    menu.createMenu(this);
    [TID_GAME_PLAYER, TID_GAME_PARTYTEXT, TID_GAME_NPC, TID_GAME_MONSTER].forEach((tid, i) => {
      menu.appendMenu(this.#deps.getText(tid), i);
    });
    this.#placeMenu = menu;
    const movers = new MoverMenu();
    movers.attach(this.env.wm.root);
    movers.onPick = (id) => {
      this.#deps.ports.actions.pickNpc(id);
      this.setFocus();
    };
    this.#moverMenu = movers;
    // `Move( rectRoot.right - rectWindow.Width(), rectRoot.top )`
    const layout = this.env.wm.layoutRect;
    this.move({ x: layout.r - rectWidth(this.rectWindowValue), y: layout.t });
    this.#applyView(this.#deps.ports.view.get());
    this.#map.redraw(this.env.host.now());
  }

  /** `OnSize`: the zoom buttons stick to the bottom-left corner, the map covers the client. */
  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    if (!this.isCreated) return;
    this.#map.setWndRect({ l: 0, t: 0, r: cx, b: cy });
    this.#place?.setWndRect({ l: 0, t: 1, r: 16, b: 17 });
    this.#zoomIn?.setWndRect({ l: 0, t: cy - 32, r: 16, b: cy - 16 });
    this.#zoomOut?.setWndRect({ l: 0, t: cy - 16, r: 16, b: cy });
  }

  // ---- the view ----------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    super.process(now);
    const v = this.#deps.ports.view.get();
    if (v !== this.#applied) this.#applyView(v);
    if (this.isVisible()) this.#map.redraw(now);
  }

  #applyView(v: NavigatorView): void {
    this.#applied = v;
    this.setTitle(v.regionName !== '' ? v.regionName : this.#deps.getText(TID_APP_NAVIGATOR));
    const f = v.filters;
    [f.player, f.party, f.npc, f.monster].forEach((on, i) => {
      this.#placeMenu?.checkMenuItem(i, on);
    });
    // `m_wndPlace.EnableWindow( !pWorld->m_bIsIndoor )`
    this.#place?.enableWindow(!v.indoor);
  }

  // ---- reg info: the filters and the zoom (`SerializeRegInfo`) -----------------------------------------------------------------

  override serializeRegInfo(): unknown {
    const v = this.#deps.ports.view.get();
    return {
      f: [v.filters.player, v.filters.party, v.filters.npc, v.filters.monster],
      z: v.zoom,
    };
  }

  override restoreRegInfo(v: unknown): void {
    if (typeof v !== 'object' || v === null) return;
    const rec = v as { f?: unknown; z?: unknown };
    const f = Array.isArray(rec.f) ? rec.f : [];
    const flag = (i: number): boolean => f[i] !== false;
    const filters: NavFilterView = {
      player: flag(0),
      party: flag(1),
      npc: flag(2),
      monster: flag(3),
    };
    const zoom = typeof rec.z === 'number' ? rec.z : EMPTY_NAVIGATOR_VIEW.zoom;
    this.#restored = { filters, zoom };
    this.#deps.ports.actions.restore(filters, zoom);
  }

  /** What `restoreRegInfo` applied last (tests). */
  get restored(): { filters: NavFilterView; zoom: number } | null {
    return this.#restored;
  }

  // ---- buttons and menus -------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      // a tick of the place menu: the item's parent is the menu (`pWndBase->m_pParentWnd == &m_wndMenuPlace`)
      if (src !== null && 'parent' in src && src.parent === this.#placeMenu && this.#placeMenu) {
        this.#deps.ports.actions.toggleFilter(id);
        return true;
      }
      if (id === NAV_ID_PLACE) {
        this.#openPlaceMenu();
        return true;
      }
      if (id === NAV_ID_ZOOM_IN) {
        this.#deps.ports.actions.zoom(1);
        return true;
      }
      if (id === NAV_ID_ZOOM_OUT) {
        this.#deps.ports.actions.zoom(-1);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** The menus open to the left of the window (to the right when there is no room): `__IMPROVE_SYSTEM_VER15`. */
  #menuSpot(menuWidth: number): Point {
    const rect = this.getScreenRect();
    const layout = this.env.wm.layoutRect;
    const left = rect.l - menuWidth;
    return { x: left < layout.l ? rect.r : left, y: rect.t };
  }

  #openPlaceMenu(): void {
    const menu = this.#placeMenu;
    if (!menu) return;
    menu.move(this.#menuSpot(rectWidth(menu.rectWindow)));
    menu.setVisible(!menu.isVisible());
    menu.setFocus();
  }

  // ---- the pointer ---------------------------------------------------------------------------------------------------------------

  override onLButtonDown(pt: Point): void {
    this.#deps.ports.actions.lButtonDown(pt, this.mapSize);
  }

  override onLButtonUp(): void {
    this.#deps.ports.actions.lButtonUp();
  }

  override onLButtonDblClk(pt: Point): void {
    this.#deps.ports.actions.doubleClick(pt, this.mapSize);
  }

  override onRButtonDown(): void {
    const npcs = this.#deps.ports.actions.rButtonDown();
    const menu = this.#moverMenu;
    if (!menu || npcs.length === 0) return;
    const spot = this.#menuSpot(160);
    menu.show(
      npcs.map((n) => ({ id: n.id, text: n.name })),
      spot,
    );
  }

  override onMouseMove(pt: Point): void {
    const hit = this.#deps.ports.actions.hover(pt, this.mapSize);
    if (!hit) return;
    const o = this.getScreenClientRect();
    this.#deps.tooltip.put(this, hit.text, rectOffset(hit.rect, o.l, o.t), 0);
  }

  override onDestroy(): void {
    this.#placeMenu?.destroy();
    this.#moverMenu?.destroy();
    super.onDestroy();
  }
}

export { rectDeflate };
