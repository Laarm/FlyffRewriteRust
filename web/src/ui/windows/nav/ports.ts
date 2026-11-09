// Ports of the navigator, teleporter, inspection and collecting windows (Plan/42 §19.4 C / D / E / L / S, WP-UI-36; wired by
// 45 WP-CFEAT-28, -29, -26, -35). The READ MODELS (`*View`, observed through a `ViewSource`) and the ACTIONS the shells call. Plain TS
// interfaces: `ui/` never imports `features/`. Every action names the legacy send behind it; the decisions (what the minimap shows, the
// texts of the teleporter, the rules of the exchange) are the features'.

import type { Point, Rect } from '../../layout/geometry';
import type { ItemView } from '../../tooltip/types';
import type { CorePreviewPort, InventoryItem } from '../char/ports';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

// ---- the navigator (APP_NAVIGATOR, 45 §7.6, ref/99 §4.4) ---------------------------------------------------------------------------------

/** The place menu (`m_bObjFilterPlayer/Party/NPC/Monster`). */
export interface NavFilterView {
  readonly player: boolean;
  readonly party: boolean;
  readonly npc: boolean;
  readonly monster: boolean;
}

export interface NavigatorView {
  /** A number that changes with every field below (the shell re-reads only on a change). */
  readonly version: number;
  /** The title: the name of the region the character entered last (`SetRegionName`); `''` = the default title. */
  readonly regionName: string;
  /** The pixel size of one land (`m_size`), 128..352. */
  readonly zoom: number;
  readonly filters: NavFilterView;
  /** An indoor world: no dots, the fog picture, the place button disabled. */
  readonly indoor: boolean;
}

export const EMPTY_NAVIGATOR_VIEW: NavigatorView = {
  version: 0,
  regionName: '',
  zoom: 256,
  filters: { player: true, party: true, npc: true, monster: true },
  indoor: false,
};

/** The size of the drawing area (`GetClientRect()`). */
export interface NavSize {
  readonly w: number;
  readonly h: number;
}

/** A name under the pointer (a mover dot or a mark): the text and the hit rect in the map's own coordinates. */
export interface NavHover {
  readonly text: string;
  readonly rect: Rect;
}

/** One entry of the popup of nearby NPCs (`__FIND_OBJ_INSIGHT`). */
export interface NavNpcEntry {
  readonly id: number;
  readonly name: string;
}

export interface NavigatorActions {
  /** `OnChildNotify` 100005 / 100006 (`ResizeMiniMap`). */
  zoom(direction: 1 | -1): void;
  /** A tick of the place menu (`OBJFILTER_*` 0..3). */
  toggleFilter(id: number): void;
  /** `OnLButtonDown`: the own mark, `SendSetNaviPoint` for a party or a focused player, then the dot under the pointer is selected. */
  lButtonDown(pt: Point, size: NavSize): void;
  /** `OnLButtonUp`: an NPC focus ends. */
  lButtonUp(): void;
  /** `OnRButtonDown`: focus and destination end; the NPCs in sight for the popup (empty = no popup). */
  rButtonDown(): readonly NavNpcEntry[];
  /** `OnLButtonDblClk` (game masters): `/teleport <world> <x> <z>` through the chat. */
  doubleClick(pt: Point, size: NavSize): void;
  /** The mover or mark under the pointer (`g_toolTip.PutToolTip`), `null` for none. */
  hover(pt: Point, size: NavSize): NavHover | null;
  /** The popup entry picked: the NPC becomes the focus and the quest destination arrow. */
  pickNpc(id: number): void;
  /** The persisted state of the window came back (`SerializeRegInfo`): the filters and the zoom. */
  restore(filters: NavFilterView, zoom: number): void;
}

export interface NavigatorPorts {
  readonly view: ViewSource<NavigatorView>;
  readonly actions: NavigatorActions;
  /** Paints the whole minimap (land tiles, dots, marks, the player arrow) into a `size` area at the origin of `ctx`. */
  paint(ctx: CanvasRenderingContext2D, size: NavSize, nowMs: number): void;
}

// ---- the world-map overlay (APP_MAP, 45 §7.7, ref/99 §4.5) ----------------------------------------------------------------------------------

/** One arrow on the world map (the own character or a party member). */
export interface WorldMapArrow {
  readonly x: number;
  readonly y: number;
  readonly name: string;
  readonly self: boolean;
  /** Degrees clockwise from north (the own arrow follows the camera). */
  readonly heading: number;
}

/** One teleporter icon with its hover state. */
export interface WorldMapIcon {
  readonly index: number;
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** One continent button of the overlay. */
export interface WorldMapButton {
  readonly rect: Rect;
  /** The texture of the atlas and the rect of the frame in it. */
  readonly sprite: {
    readonly path: string;
    readonly x: number;
    readonly y: number;
    readonly w: number;
    readonly h: number;
  } | null;
}

/** What the overlay draws (the model is built by `features/navigator` from the bundles and the game state). */
export interface WorldMapView {
  readonly open: boolean;
  /** The 4:3 box in the viewport. */
  readonly box: Rect;
  readonly rate: number;
  /** `m_nMap` (0..7). */
  readonly mapNo: number;
  /** The path of the map texture (`theme/world_*.dds`). */
  readonly texture: string | null;
  /** The continent buttons: the screen rect and the cut of `texMapButton.inc` (the frame already follows hover and selection). */
  readonly buttons: readonly WorldMapButton[];
  readonly arrows: readonly WorldMapArrow[];
  readonly icons: readonly WorldMapIcon[];
  /** The teleporter mode (`m_bTeleportMode`): icons show and a click on one teleports. */
  readonly teleporter: boolean;
  /** The icon under the pointer (drawn 20 x 20). */
  readonly hoverIcon: number | null;
  /** The pixel size of the viewport (for the arrow size). */
  readonly viewH: number;
  /**
   * The destination sprite (`CWorldMap::m_rectDestination`, `ButtDestination.bmp` 16 x 16 scaled 0.6, alpha pulsing 0 <-> 255 at 800 per second) in destination mode: the top-left of the rect
   * centred on the destination, `null` when the destination is not on the shown map (45 WP-CFEAT-42; absent = none).
   */
  readonly destination?: { readonly x: number; readonly y: number } | null;
}

export interface WorldMapPorts {
  readonly view: ViewSource<WorldMapView>;
  /** The pointer moved over the overlay (virtual UI pixels). */
  hover(x: number, y: number): void;
  /** The applet `M` (`CWndMap::Initialize`): opens the overlay, or closes it when it is shown. */
  toggle(): void;
}

// ---- the TELE16 teleporter (APP_TELE16, 45 §26) ----------------------------------------------------------------------------------------

/** One drop of the boss panel (a `II_*` item id and its icon). */
export interface Tele16Drop {
  readonly itemId: number;
  readonly icon: string | null;
  readonly view: ItemView;
}

/** The boss panel of an entry whose `boss` is set. */
export interface Tele16Boss {
  readonly name: string;
  readonly levelText: string;
  readonly healthText: string;
  /** The element icon (`m_texAttrIcon` frame, 0 = none). */
  readonly element: number;
  readonly elementText: string;
  readonly reqText: string;
  readonly cooldownText: string;
  readonly drops: readonly Tele16Drop[];
}

/** One listed entry. `index` is the ROW index of `teleport16.inc` (what `Tele16` sends; the list position differs when a row has no name). */
export interface Tele16Entry {
  readonly index: number;
  readonly name: string;
  readonly desc: string;
  /** The picture (a bare file name under `Theme/`), `''` = the default picture. */
  readonly image: string;
  readonly boss: Tele16Boss | null;
}

export interface Tele16View {
  readonly entries: readonly Tele16Entry[];
}

export interface Tele16Actions {
  /** `SendTeleport( m_nCursorIndex )`. */
  teleport(index: number): void;
}

export interface Tele16Ports {
  readonly view: ViewSource<Tele16View>;
  readonly actions: Tele16Actions;
}

// ---- the inspection window (APP_QUERYEQUIP, 45 §9.8) ---------------------------------------------------------------------------------

/** One worn item of the inspected player with the record the tooltip composer reads. */
export interface InspectItem extends InventoryItem, ItemView {
  /** The worn part (`PARTS_*`). */
  readonly part: number;
}

export interface QueryEquipView {
  readonly version: number;
  /** The inspected player's name (the title shows it after "View"). */
  readonly name: string;
  /** The inspected mover is gone: the window closes (`IsInvalidObj → Destroy`). */
  readonly gone: boolean;
  /** The worn items by `PARTS_*` (31 entries). */
  readonly equip: readonly (InspectItem | null)[];
  /** The 3-D preview subject (`MoverVisualState` of the inspected player). */
  readonly subject: unknown;
  /** Per-slot hide flags of the fashion pieces (`m_abHideCoat`). */
  readonly hideCoat: readonly boolean[];
}

export interface QueryEquipPorts {
  readonly view: ViewSource<QueryEquipView>;
  /** The 3-D preview of the inspected player (null without WebGL). */
  readonly previews?: CorePreviewPort | null;
  /**
   * The tooltips of the window are about the INSPECTED player (the requirement lines): `true` while the pointer is over an item of the
   * window, `false` when it leaves. The window calls it only on a change.
   */
  tooltipScope?(on: boolean): void;
  /** The window closed (the inspected owner of the tooltips is dropped). */
  closed?(): void;
}

// ---- the collecting window (APP_COLLECTING, 45 §9.8) ---------------------------------------------------------------------------------

export interface CollectingView {
  readonly version: number;
  /** The collector is in the hand and shown (`GetCollector() != NULL`). */
  readonly hasCollector: boolean;
  /** `GetAbilityOption()` of the collector (0..5). */
  readonly level: number;
  /** `m_nHitPoint` of the collector (the battery), clamped at 0. */
  readonly battery: number;
  /** `GetMaxBattery()`. */
  readonly batteryMax: number;
  /** The collecting animation runs (`StartCollecting`): the button reads "Stop". */
  readonly collecting: boolean;
}

export const EMPTY_COLLECTING_VIEW: CollectingView = {
  version: 0,
  hasCollector: false,
  level: 0,
  battery: 0,
  batteryMax: 1,
  collecting: false,
};

export interface CollectingActions {
  /** The start button: `SendQueryStartCollecting` (refused while flying with `TID_GAME_COLLECT_FLY`) or `SendQueryStopCollecting`. */
  toggle(): void;
}

export interface CollectingPorts {
  readonly view: ViewSource<CollectingView>;
  readonly actions: CollectingActions;
}

// ---- shared: the rects the shells use ---------------------------------------------------------------------------------------------------

/** A point of a hover event in the window's client coordinates. */
export type ClientPoint = Point;
