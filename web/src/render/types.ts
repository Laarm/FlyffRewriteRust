// Types shared by the renderer modules and its consumers (Plan/41 sections 2.2, 5.1, 20, 21). Interfaces whose
// owner is another work package are declared here in their minimal form (marked PROVISIONAL) so that the
// facade compiles before the owner lands; the owner refines them.

import type { Matrix4 } from 'three';
import type { MoverVisualState } from '../game/entities';
import type { ContinentDef, WorldSettingsDto } from '../gen/bundles/types';
import type { Vec3 } from './space';

/** Entity id on the wire and in the entity table. */
export type ObjId = number;

/** A position/direction in the LEFT-HANDED game space (+Y up, +Z north). */
export type Vec3LH = readonly [number, number, number];

/** Camera state produced by doc 43, LH (Plan/41 section 5.1). */
export interface CameraState {
  /** Eye position (CBackCamera result incl. collision), interpolated (PL-009). */
  readonly pos: Vec3LH;
  readonly lookAt: Vec3LH;
  readonly up: Vec3LH;
  /** `fDiv`: 4 normal; 10/25/50 telescope 1-3; 2..4 flying turbo/accel (ref/21 section 6.4). */
  readonly fovDiv: number;
  /** Zoom level (own model and own name hidden when not 0). */
  readonly telescope: 0 | 1 | 2 | 3;
  /** `m_fCurRotx`: sky yaw-only view, audio listener. */
  readonly yawDeg: number;
  readonly mode: 'normal' | 'myroom';
}

/** The game clock as the original keeps it (Plan/41 section 8.7): hour 1..24, minute, second. */
export interface GameClockSample {
  readonly hour: number;
  readonly min: number;
  readonly sec: number;
}

/**
 * PROVISIONAL (WP-REND-12 / doc 40 section 7.5): the reader over `SimWorld.render_view`, valid for the current
 * frame only (B4). The base renderer never reads it.
 */
export interface RenderViewReaderLike {
  readonly frame: number;
}

/** What the renderer reads of an entity record (`game/entities.ts` `EntityRecord` satisfies it). */
export interface EntityViewRecord {
  readonly objId: ObjId;
  readonly isSelf: boolean;
  /** `DelObj` was processed; the record is deleted at the end of the drain. */
  readonly removed: boolean;
  readonly hp: number;
  /** Movers only (Plan/41 12.1). */
  readonly visual?: MoverVisualState | undefined;
}

/** Doc 40 section 6.3: ObjId -> entity record (`EntityTable`); the mover batcher iterates it. */
export interface EntityViewSource {
  get(id: ObjId): unknown;
  /** Iterates the live entities (`0` = every kind); absent on the dev pages that have no entity table. */
  forEach?(kind: 0, fn: (r: EntityViewRecord) => void): void;
  /** The own character's record (the viewer of the frame). */
  self?(): EntityViewRecord | undefined;
  /** Bumps whenever an entity is added or deleted. */
  readonly version?: number;
}

/** What `WorldRenderer.draw` receives once per rendered iteration (Plan/41 section 2.2). */
export interface FrameInput {
  readonly view: RenderViewReaderLike | null;
  readonly camera: CameraState;
  readonly gameClock: GameClockSample;
  /** `performance.now()`, for millisecond timers (balloons, eye blink). */
  readonly nowMs: number;
  readonly entities: EntityViewSource | null;
  /**
   * The ACTIVE mover's (the player's) logical position in LH space: the centre of the terrain streaming window and the
   * occluder of the static-object fade (Plan/41 9.8, 11.6). Defaults to `camera.lookAt` (the back camera looks at the
   * player); a free-fly viewer passes the camera position.
   */
  readonly activePos?: Vec3LH;
  /**
   * The server weather as the stores hold it (`worldStore.env`: `EnvironmentAll.season`, `Environment.effect`, Plan/41 15); the
   * renderer detects the changes. Absent on the dev pages (no weather).
   */
  readonly weather?: { readonly season: number; readonly effect: boolean };
}

/** The world the renderer shows (`setWorld`, Plan/41 section 2.2). */
export interface WorldRenderDesc {
  readonly worldId: number;
  readonly folder: string;
  readonly settings: WorldSettingsDto;
  readonly continents: readonly ContinentDef[];
  readonly mpu: number;
  readonly landsW: number;
  readonly landsH: number;
  readonly indoor: boolean;
  /** Land indices (`x + z * landsW`) that have a `.lnd` file. */
  readonly present: readonly number[];
}

/** Routed `SimWorld` events (40 section 7.6). PROVISIONAL: the cosmetic kinds are added by 41's later WPs. */
export interface RenderEvent {
  readonly kind: string;
  readonly id?: ObjId;
  readonly data?: unknown;
}

/**
 * A drawn object's pick proxy (Plan/41 section 20.2, WP-REND-15). `worldMatrix` is the LOGICAL placement
 * `Scale * Rotation * Translate(pos)` in three.js space (a pet: its screen position), `localAabb` the model box in the model
 * space of that matrix (players: the union of the parts, 12.3 item 7). Doc 43 applies the candidate filters and the closest-hit
 * rule; the own player is in the list (43 excludes it).
 */
export interface PickProxy {
  readonly id: ObjId;
  readonly kind: 'mover' | 'item' | 'ctrl';
  readonly worldMatrix: Matrix4;
  readonly localAabb: readonly [Vec3, Vec3];
  readonly dead: boolean;
  readonly isPlayer: boolean;
  /** `MoverProp.dwClass` (`RANK_*`); 0 for items and ctrls. */
  readonly rank: number;
  /** `MoverProp.dwAI`; 0 for items and ctrls. */
  readonly ai: number;
  /** The own system pet (client-only pets are skipped unless they are the own one). */
  readonly ownPet: boolean;
  /** GM-invisible (`TRANSPARENT_MODE`). */
  readonly transparent: boolean;
  /** `GCWAR_NOT_CLICK_MODE`: the pick returns nothing. */
  readonly gcwarNotClick: boolean;
}

/** A screen rectangle in CSS pixels (Plan/41 section 20.4). */
export interface ScreenRect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** A ray in three.js space. */
export interface Ray3 {
  readonly origin: readonly [number, number, number];
  readonly dir: readonly [number, number, number];
}

/** The world HUD gauges (Plan/41 section 2.2 `setGauges`). */
export interface GaugeState {
  charge: 0 | 1 | 2 | 3 | 4;
  casting?: [number, number];
  collecting?: [number, number];
  /** The turbo bar of a flying player: `[m_tmAccFuel, 12000]` (WP-REND-25). */
  fuel?: [number, number];
  /** The turbo is on (`OBJSTAF_TURBO`): the bar blinks per tick. */
  turbo?: boolean;
  /** `g_nFlySpeed`: the digits next to the bar (`ftoi(ftoi(|v| * 60^3) / 200)`). */
  speed?: number;
}

/** Target brackets (Plan/41 section 2.2 `setTargets`). */
/** `focus` / `next` are `0` when there is no target. */
export interface TargetState {
  focus: ObjId;
  next: ObjId;
  red: boolean;
  flyTargets: readonly ObjId[];
}

/** An RGB triple with components in 0..1 (gamma-naive: the numbers are the original 8-bit values / 255). */
export type RGB = readonly [number, number, number];
