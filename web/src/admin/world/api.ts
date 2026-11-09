// What the other parts of the console may use of the live map: the inspector, the tools, the locations panel and the context menus
// (WP-ADMIN-13 and -14) receive a `MapApi` as a prop of their slot component (`views/world/slots.tsx`). Import this file with `import type`:
// the runtime of the map is in the lazy chunk, this contract is plain types, so a slot component never pulls the engine into its own chunk.
//
//     function Inspector({ api }: MapSlotProps) {
//       const selection = useStore(api.store, (s) => s.selection);       // `zustand`'s `useStore` over the vanilla store
//       const e = api.entity(selection[0]);                              // a snapshot of the entity, or null while it is not in view
//       ...
//     }
import type { ComponentType } from 'react';
import type { MapStore, ToolId } from './state';
import type { Place } from '../components/mapPlace';
import type { Highlight } from './overlays';
import type { FindHit, MetaDoc } from './metaTypes';

/** A snapshot of one entity of the latest frames (positions are the CURRENT ones, not the interpolated ones). */
export interface EntityInfo {
  readonly id: number;
  /** `EntityKind`: 1 player, 2 monster, 3 NPC, 4 pet, 5 item, 6 ctrl. */
  readonly kind: number;
  /** A player's job id, a monster's rank code (0 normal .. 5 guard), a ctrl's kind. */
  readonly sub: number;
  readonly prop: number;
  /** An item's stack count. */
  readonly level: number;
  readonly name: string;
  readonly x: number;
  readonly z: number;
  /** `ceil(hp * 100 / max)`, 0 when dead. */
  readonly hpPct: number;
  /** `MapFlags` bits. */
  readonly flags: number;
  /** The object this monster targets (tier A), 0 for none. */
  readonly target: number;
  /** A player's character id from the online roster (0 when unknown). */
  readonly characterId: number;
  readonly guildId: number;
}

/** A point the user picked on the map ("Teleport to point...", "Spawn here..."). */
export interface PickedPoint {
  readonly world: number;
  readonly layer: number;
  readonly x: number;
  readonly z: number;
}

/** A right click (or long press) on the map. */
export interface MapContextEvent {
  /** Client coordinates (where to open the menu). */
  readonly clientX: number;
  readonly clientY: number;
  readonly world: number;
  readonly layer: number;
  readonly x: number;
  readonly z: number;
  /** The entity under the pointer, if any. */
  readonly entity: EntityInfo | null;
}

export interface MapApi {
  /** The interface state (`useStore(api.store, selector)`): world, layer, selection, follow, view, counts, tier, ... */
  readonly store: MapStore;
  /** The meta document of the world on screen. */
  meta(): MetaDoc | null;
  /** The entity with this object id in the latest frames. */
  entity(id: number): EntityInfo | null;
  /** The entities of the latest frames inside the canvas (`kind` filters by `EntityKind`), players first. */
  inView(kind?: number): EntityInfo[];
  /** Selects entities (`set` replaces, `add` adds, `toggle` flips). */
  select(ids: readonly number[], mode?: 'set' | 'add' | 'toggle'): void;
  clearSelection(): void;
  /** Follows an entity (`null` stops). A player is followed across worlds and layers. */
  follow(id: number | null): void;
  /** Opens a world and layer and flies to a point (350 ms ease). Resolves when the world is loaded. */
  flyTo(place: Place): Promise<void>;
  /** Centres on the acting character (key M); `false` when it is not online. */
  jumpToMe(): boolean;
  /** The next click on the map answers with a point (Esc cancels with `null`). The map shows `prompt` in its status bar. */
  pickPoint(prompt: string): Promise<PickedPoint | null>;
  /** Shapes drawn on the map under a key of the caller (a spawn region of the monster card, a bookmark): `[]` removes them. */
  setHighlights(key: string, items: readonly Highlight[]): void;
  /** Right clicks on the map. The first listener that returns `true` has handled the event (the browser menu stays off). */
  onContextMenu(fn: (e: MapContextEvent) => unknown): () => void;
  /** The favourite keys 1..9 (`index` 0..8) and `/`-style keys the map does not use itself. */
  onFavourite(fn: (index: number) => void): () => void;
  /** The pointer tool: `pan`, `select` (box), `ruler`, or `pick` (set by `pickPoint`). */
  setTool(tool: ToolId): void;
  /** The ruler's next point (the first click sets `a`, the second `b`, the third starts over): "Measure from here" of the context menu. */
  rulerPoint(x: number, z: number): void;
  /** Runs the find on the map (`GET /admin/api/world/find`) and reveals the hit. */
  reveal(hit: FindHit): Promise<void>;
  /** Closes the live subscription for good and opens the "Resume live view" overlay (the console logs the user out, a modal opens, ...). */
  pause(): void;
  resume(): void;
}

export interface MapSlotProps {
  readonly api: MapApi;
}

/** The parts of the map that other tasks fill (`views/world/slots.tsx`). A missing slot shows the built-in default. */
export interface MapSlots {
  /** The Locations panel at the left of the canvas (200 px, a 44 px rail below 1440 px; `rail` says which). */
  readonly locations?: ComponentType<
    MapSlotProps & { readonly rail: boolean; readonly onToggleRail: () => void }
  >;
  /** The Inspector tab of the sidebar. */
  readonly inspector?: ComponentType<MapSlotProps>;
  /** The Tools tab of the sidebar. */
  readonly tools?: ComponentType<MapSlotProps>;
  /** Drawn over the canvas, inside the map area: the hover card, the context menu and the "You" marker. It must not take the pointer
   *  outside what it draws (`pointer-events: none` on its root). */
  readonly overlay?: ComponentType<MapSlotProps>;
}
