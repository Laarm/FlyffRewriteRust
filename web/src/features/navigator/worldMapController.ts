// The world-map overlay (`CWorldMap`, `src:Neuz/WorldMap.cpp`; ref/99 §4.5; Plan/45 §7.7; WP-CFEAT-28): its state (shown, the map number, the
// teleporter mode), the click and hover rules and the `WorldMapView` the overlay layer draws. The geometry is `worldMap.ts`, the data is
// `worlds.world_maps` and the continent polygons; the game state (position, camera, party, viewport) comes in through `WorldMapEnv`.
//
//   open (key M)     `LoadWorldMap`: the map of the continent the player stands in; outside Madrigal or outside every continent the map
//                    number is 100 and the overlay does NOT open, the player gets `TID_GAME_MAPMSG`
//   teleporter       `SetTelMode( TRUE ); LoadWorldMap(); ProcessingTeleporter( npc )`: the whole-world map (map 0), icons at the NPC's
//                    destinations that lie on the shown map; a click on an icon sends `Teleporter{key, index}` and closes the overlay
//   buttons          the nine continent buttons: the first eight select a map (icons are recomputed), the ninth is the monster info (M8)
//   Esc              closes (the input layer asks `shown()` / `hide()`); a click anywhere else is eaten

import type { WorldMapRow } from '../../gen/bundles';
import type { WorldMapPorts, WorldMapView, ViewSource } from '../../ui/windows/nav/ports';
import { type ContinentPoly, MAP_NONE, mapNumber } from './continent';
import {
  type BoxRect,
  BUTTON_COUNT,
  buttonAt,
  buttonFrame,
  ICON_HOVER_SIZE,
  iconAt,
  mapBox,
  mapButtons,
  teleportIcons,
  type TeleportIcon,
  worldToMap,
} from './worldMap';

/** One party arrow source. */
export interface WorldMapMember {
  readonly name: string;
  readonly x: number;
  readonly z: number;
}

export interface WorldMapEnv {
  readonly maps: () => readonly WorldMapRow[];
  readonly polys: () => readonly ContinentPoly[];
  /** The virtual UI viewport (V of the scaler). */
  readonly viewport: () => { w: number; h: number };
  readonly worldId: () => number;
  /** The own character: position, the heading of the camera in degrees, the name; `null` outside a world. */
  readonly self: () => { x: number; z: number; heading: number; name: string } | null;
  /** The party members except the own character that are online. */
  readonly members: () => readonly WorldMapMember[];
  /** The rect of frame `i` of `texMapButton.inc` and the path of its texture. */
  readonly buttonSprite: (
    frame: number,
  ) => { path: string; x: number; y: number; w: number; h: number } | null;
  /** `Teleporter{key, index}`. */
  readonly sendTeleporter: (key: string, index: number) => void;
  /** `PLAYSND( SND_INF_CLICK )`. */
  readonly click?: () => void;
  /** `g_WndMng.PutString( TID_GAME_MAPMSG )`. */
  readonly noMapMessage: () => void;
  readonly now: () => number;
  /** The destination arrow (`m_vDestinationArrow`), `null` for none (destination mode, 45 WP-CFEAT-42; absent = none). */
  readonly destination?: () => { x: number; z: number } | null;
}

/** The view of a closed overlay (one object: the layer renders nothing). */
const CLOSED: WorldMapView = {
  open: false,
  box: { l: 0, t: 0, r: 0, b: 0 },
  rate: 1,
  mapNo: 0,
  texture: null,
  buttons: [],
  arrows: [],
  icons: [],
  teleporter: false,
  hoverIcon: null,
  viewH: 0,
  destination: null,
};

interface TeleMode {
  readonly key: string;
  readonly positions: readonly (readonly [number, number])[];
}

export class WorldMapController {
  readonly #env: WorldMapEnv;
  readonly #listeners = new Set<() => void>();
  #open = false;
  #mapNo = 0;
  #tele: TeleMode | null = null;
  /** `m_bDestinationMode`: the quest destination is drawn (45 WP-CFEAT-42). */
  #destMode = false;
  #hover: { x: number; y: number } | null = null;
  #showMonsters = false;
  #view: WorldMapView | null = null;
  #key = '';
  /** What the controller sent / did, for the probe and the tests. */
  readonly log: string[] = [];

  constructor(env: WorldMapEnv) {
    this.#env = env;
  }

  get shown(): boolean {
    return this.#open;
  }
  get mapNo(): number {
    return this.#mapNo;
  }
  get teleporterMode(): boolean {
    return this.#tele !== null;
  }
  get monsterInfo(): boolean {
    return this.#showMonsters;
  }
  get destinationMode(): boolean {
    return this.#destMode;
  }

  // ---- open / close --------------------------------------------------------------------------------------------------------

  /** `CWndMap::Initialize`: the overlay toggles. */
  toggle(): boolean {
    if (this.#open) {
      this.close();
      return false;
    }
    return this.open();
  }

  /** `LoadWorldMap` for the plain map (key M). Returns whether it opened. */
  open(): boolean {
    const self = this.#env.self();
    if (!self) return false;
    const no = mapNumber(this.#env.worldId(), self.x, self.z, this.#env.polys(), this.#env.maps());
    if (no === MAP_NONE) {
      this.#env.noMapMessage();
      this.log.push('nomap');
      return false;
    }
    this.#tele = null;
    this.#destMode = false;
    this.#mapNo = no;
    this.#set(true);
    return true;
  }

  /**
   * `SetDestinationMode( TRUE ); LoadWorldMap(); UpdateDestinationPosition( dest )` (`ProcessQuestDestinationWorldMap`): the map of the continent the player stands in with the sprite of the
   * destination. The overlay does not open where there is no map (the same refusal as the key M).
   */
  openDestination(): boolean {
    if (!this.open()) return false;
    this.#destMode = true;
    this.#key = '';
    this.#notify();
    return true;
  }

  /** `SetTelMode( TRUE ); LoadWorldMap(); ProcessingTeleporter( npc )`: the whole world with the NPC's destinations. */
  openTeleporter(key: string, positions: readonly (readonly [number, number])[]): boolean {
    if (positions.length === 0) return false;
    this.#tele = { key, positions };
    this.#mapNo = 0; // `nMap = CONT_NODATA`
    this.#showMonsters = false;
    this.#set(true);
    return true;
  }

  /** `DeleteWorldMap`: closes the overlay and ends the teleporter mode. */
  close(): void {
    if (!this.#open && this.#tele === null) return;
    this.#tele = null;
    this.#destMode = false;
    this.#hover = null;
    this.#set(false);
  }

  #set(open: boolean): void {
    this.#open = open;
    this.#key = '';
    this.#notify();
  }

  // ---- the pointer -----------------------------------------------------------------------------------------------------------

  /** The pointer moved (virtual UI pixels): the hovered icon and button. */
  hover(x: number, y: number): void {
    if (!this.#open) return;
    const prev = this.#hover;
    if (prev?.x === x && prev.y === y) return;
    this.#hover = { x, y };
    this.#notify();
  }

  /** `OnLButtonDown` (virtual UI pixels). Returns whether the click did something. */
  click(x: number, y: number): boolean {
    if (!this.#open) return false;
    const vp = this.#env.viewport();
    const tele = this.#tele;
    if (tele) {
      const icon = iconAt(this.#icons(), x, y);
      if (icon) {
        this.#env.click?.();
        this.#env.sendTeleporter(tele.key, icon.index);
        this.log.push(`teleporter:${tele.key}:${String(icon.index)}`);
        this.close();
        return true;
      }
    }
    const b = buttonAt(mapButtons(vp.w, vp.h), x, y);
    if (b) {
      const maps = this.#env.maps();
      if (b.index < Math.min(BUTTON_COUNT - 1, maps.length)) {
        this.#mapNo = b.index;
        this.#notify();
      } else if (b.index === BUTTON_COUNT - 1) {
        this.#showMonsters = !this.#showMonsters;
        this.#notify();
      }
      return true;
    }
    return true; // the overlay eats every left click
  }

  // ---- the model ---------------------------------------------------------------------------------------------------------------

  #box(): { box: BoxRect; rate: number } {
    const vp = this.#env.viewport();
    return mapBox(vp.w, vp.h);
  }

  #icons(): TeleportIcon[] {
    const tele = this.#tele;
    if (!tele) return [];
    const { box } = this.#box();
    return teleportIcons(
      tele.positions,
      this.#env.maps(),
      this.#mapNo,
      this.#env.worldId(),
      this.#env.polys(),
      box,
    );
  }

  /** The view for the overlay layer; the same object while nothing it shows changed. */
  view(): WorldMapView {
    if (!this.#open) return CLOSED;
    const vp = this.#env.viewport();
    const { box, rate } = this.#box();
    const maps = this.#env.maps();
    const polys = this.#env.polys();
    const world = this.#env.worldId();
    const icons = this.#icons();
    const hover = this.#hover;
    const hoverIcon = hover ? (iconAt(icons, hover.x, hover.y)?.index ?? null) : null;
    const buttons = mapButtons(vp.w, vp.h).map((b) => {
      const over =
        hover !== null &&
        hover.x >= b.x &&
        hover.x < b.x + b.w &&
        hover.y >= b.y &&
        hover.y < b.y + b.h;
      const frame = buttonFrame(b.index, this.#mapNo, over);
      return {
        rect: { l: b.x, t: b.y, r: b.x + b.w, b: b.y + b.h },
        sprite: this.#env.buttonSprite(frame),
      };
    });
    const arrows: WorldMapView['arrows'][number][] = [];
    const self = this.#env.self();
    if (self) {
      const at = worldToMap(maps, this.#mapNo, world, self.x, self.z, polys, box);
      if (at) arrows.push({ x: at.x, y: at.y, name: self.name, self: true, heading: self.heading });
      for (const m of this.#env.members()) {
        const p = worldToMap(maps, this.#mapNo, world, m.x, m.z, polys, box);
        if (p) arrows.push({ x: p.x, y: p.y, name: m.name, self: false, heading: 0 });
      }
    }
    // destination mode: the sprite (16 x 16) centred on the destination, recomputed whenever the shown map changes
    let destination: { x: number; y: number } | null = null;
    const dest = this.#destMode ? this.#env.destination?.() : null;
    if (dest) {
      const at = worldToMap(maps, this.#mapNo, world, dest.x, dest.z, polys, box);
      if (at) destination = { x: Math.trunc(at.x - 8), y: Math.trunc(at.y - 8) };
    }
    const next: WorldMapView = {
      open: this.#open,
      box: { l: box.x, t: box.y, r: box.x + box.w, b: box.y + box.h },
      rate,
      mapNo: this.#mapNo,
      texture: maps.find((m) => m.map === this.#mapNo)?.texture ?? null,
      buttons,
      arrows,
      icons: icons.map((i) => {
        const hot = hoverIcon === i.index;
        const grow = hot ? (ICON_HOVER_SIZE - i.w) / 2 : 0;
        return {
          index: i.index,
          x: i.x - grow,
          y: i.y - grow,
          w: i.w + grow * 2,
          h: i.h + grow * 2,
        };
      }),
      teleporter: this.#tele !== null,
      hoverIcon,
      viewH: vp.h,
      destination,
    };
    const key = JSON.stringify(next);
    if (key === this.#key && this.#view) return this.#view;
    this.#key = key;
    this.#view = next;
    return next;
  }

  /** The 10 Hz refresh while the overlay is shown (the own arrow and the party arrows move). */
  refresh(): void {
    if (!this.#open) return;
    const before = this.#key;
    this.view();
    if (this.#key !== before) this.#notify();
  }

  #notify(): void {
    for (const fn of [...this.#listeners]) fn();
  }

  /** The ports the UI service takes. */
  ports(): WorldMapPorts {
    const source: ViewSource<WorldMapView> = {
      get: () => this.view(),
      subscribe: (fn) => {
        this.#listeners.add(fn);
        return () => {
          this.#listeners.delete(fn);
        };
      },
    };
    return {
      view: source,
      hover: (x, y) => {
        this.hover(x, y);
      },
      toggle: () => {
        this.toggle();
      },
    };
  }
}
