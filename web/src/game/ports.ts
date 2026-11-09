// Ports of the game loop to subsystems owned by other documents (Plan/40 section 10.2): the sim bridge
// (WP-CLIENT-18), input and camera (43), the renderer (41), audio (44), the UI (42) and the move
// reporter / pose feeder (WP-CLIENT-18/20). The loop calls these interfaces only; the real APIs of 41-44
// are supersets of them, and the null implementations below let the app compile and the loop run before
// those documents land. PROVISIONAL: the owning WPs may refine a method signature when they integrate; the
// call ORDER of section 10.2 is what is normative.

import type { AudioCues } from '../audio/cues';
import type { RenderFx } from '../render/fx';
import type { PickProxy, Ray3, ScreenRect, Vec3LH, WorldRenderDesc } from '../render/types';
import type { Vec3Tuple } from '../stores/self';
import type { RenderViewReader } from './renderView';
import type { SimCmdWriter, Vec3 } from './simCmd';
import type { SimEventRouter } from './simEvents';
import { NullSim } from './nullSim';

/** Result of one `sim.frame` call (Plan/40 section 7.3, 31 section 17.1). */
export interface SimFrameResult {
  /** Frames the sim actually stepped (<= the `due` that was asked for). */
  readonly stepped: number;
  /** A fresh render view was produced (`render_view` ran). */
  readonly viewFresh: boolean;
  /** The render view (opaque to the loop; valid until the next wasm call, B4). */
  readonly view: unknown;
  /** Sim events of this frame (<= 256, 31 section 17.6). */
  readonly events: readonly unknown[];
}

/** `set_world([world_id, mpu, lands_w, lands_h, fly_allowed, housing])` (Plan/31 section 17.3). */
export interface SimWorldDesc {
  readonly worldId: number;
  readonly mpu: number;
  readonly landsW: number;
  readonly landsH: number;
  readonly flyAllowed: boolean;
  readonly housing: boolean;
}

/** A queued bulk load into the sim (cap L, applied at step 3 of the loop, Plan/40 section 7.3). */
export type SimLoad =
  | {
      readonly kind: 'chunk';
      readonly lx: number;
      readonly lz: number;
      readonly bytes: Uint8Array;
      readonly prio: number;
    }
  | { readonly kind: 'unchunk'; readonly lx: number; readonly lz: number; readonly prio: number }
  | {
      readonly kind: 'skeleton';
      readonly id: number;
      readonly bytes: Uint8Array;
      readonly prio: number;
    }
  | {
      readonly kind: 'clip';
      readonly id: number;
      readonly bytes: Uint8Array;
      readonly prio: number;
    };

/** `{crash, out, len}` of `camera_check_bound` (43 section 8.4). */
export interface CameraBound {
  readonly crash: boolean;
  readonly out: Vec3;
  readonly len: number;
}

/** A ray hit (`ray_cast`, 31 section 17.3): `kind` 1 terrain, 2 static, 3 ctrl. */
export interface RayHit {
  readonly pos: Vec3;
  readonly t: number;
  readonly kind: number;
  readonly id: number;
  readonly normal: Vec3;
}

export interface BlinkResult {
  readonly pos: Vec3;
  readonly ok: boolean;
}

/** A queued call of the R category (`RulesContext`, WP-CLIENT-30): runs at the next `runRules`. */
export interface RulesItem {
  readonly name: string;
  run(): void;
  /** Ignored by the bridge since 2026-10-29: every queued item runs at the next `runRules`, FIFO (it used to jump the one-item queue). */
  readonly urgentAfterMs?: number;
}

/** The boundary-call counters, telemetry (`__flyffDebug.loop()`, Plan/40 section 7.3). */
export interface SimStats {
  readonly callsThisFrame: number;
  readonly maxCalls: number;
  readonly byCategory: Readonly<Record<'F' | 'C' | 'Q' | 'L' | 'R', number>>;
  readonly overBudget: number;
  readonly uiCalls: number;
  readonly eventsDropped: number;
  /** `memory.buffer.byteLength`, sampled at 1 Hz. */
  readonly memBytes: number;
  /** Collision chunks loaded in the sim. */
  readonly chunksLoaded: number;
  /** Loads still queued (chunks, skeletons, clips). */
  readonly loadsPending: number;
  readonly commandsApplied: number;
  readonly commandErrors: number;
}

/** Result of one `sim.frame` call (Plan/40 section 7.3, 31 section 17.1). */
export interface SimFrameResult {
  /** Frames the sim actually stepped (<= the `due` that was asked for). */
  readonly stepped: number;
  /** A fresh render view was produced (`render_view` ran). */
  readonly viewFresh: boolean;
  /** The render view (a `RenderViewReader` of the real bridge; valid until the next wasm call, B4). */
  readonly view: unknown;
  /** Sim events of this frame (<= 256, 31 section 17.6). */
  readonly events: readonly unknown[];
}

/** The sim bridge (Plan/40 section 7.3): the only caller of `SimWorld`; it counts the boundary calls per iteration and refuses none. */
export interface SimBridge {
  // ---- the loop's part ---------------------------------------------------------------------------------------
  /**
   * Step 3 (loads and maintenance) and step 4 of the loop: `apply_commands`, `local_input`, `step(due)`,
   * `take_events`, `render_view` (at most 5 wasm calls). `alpha` is the render interpolation factor, `1.0` for
   * a non-rendered iteration that stepped, `null` when nothing is rendered and nothing stepped.
   */
  frame(due: number, record: unknown, alpha: number | null): SimFrameResult;
  /** `frameSkew = clientFrame - simFrame` changed (the loop dropped backlog); re-sends `SET_CLOCK` (CA9). */
  setFrameSkew(skew: number): void;
  /** Remote movers snap to their newest state (hidden-tab return, own `SetPos`; a queued L item). */
  resetRenderSmoothing(): void;
  /** End of the loop iteration: folds the call counters into the statistics (step 12). */
  endFrame?(): void;
  /**
   * Test hook (`__flyffDebug.test.forceInput`): the input record is `[buttons, 0, 0, 1, 0]` for the next `durationMs`, then one
   * released record follows. `INPUT_BUTTONS` bits of `gen/sim/wire.ts`; 43's own record is ignored meanwhile.
   */
  forceInput(buttons: number, durationMs: number): void;

  // ---- lifecycle (L) -----------------------------------------------------------------------------------------
  /** `set_world`, immediately (never deferred), truncates the command buffer keeping `SET_CLOCK`. */
  setWorld(desc: SimWorldDesc): void;
  /** `set_world([0, ...])`: no world (logout, kick, scope reset). */
  leaveWorld(): void;
  /** The world the sim was told about (0 = none). */
  readonly worldId: number;
  /** One-time session inputs (Plan/40 section 3.5): the `/gen` binary files, bytes unchanged. */
  loadMotionTable(bytes: ArrayBuffer | Uint8Array): void;
  loadModelBounds(bytes: ArrayBuffer | Uint8Array): void;
  loadCtrlModels(bytes: ArrayBuffer | Uint8Array): void;
  /** `true` once `load_model_bounds` ran: a `SET_PARAMS` before it is rejected by the sim (unknown bounds entry). */
  readonly modelBoundsLoaded: boolean;

  // ---- commands and loads ------------------------------------------------------------------------------------
  /** The command writer (31 section 17.4); the bytes are applied at the next `frame`. */
  readonly cmd: SimCmdWriter;
  /** Queues a chunk / skeleton / clip load; every queued load runs at the next iteration (no cap, `Prio` then FIFO). */
  enqueueLoad(load: SimLoad): void;
  /** Queues one `RulesContext` call (WP-CLIENT-30); every queued item runs at the next `runRules`. */
  enqueueRules(item: RulesItem): void;
  /** Runs every queued R item, FIFO (items queued meanwhile wait for the next call); `true` when one ran. */
  runRules(): boolean;

  // ---- queries (category Q; `undefined` = the export is missing; LH coordinates) ---------------------------------
  /** No limit: returns `Number.MAX_SAFE_INTEGER` (queries are never refused, owner 2026-10-29); kept for the callers that still ask. */
  queryBudget(): number;
  localAct(msg: number, p: readonly [number, number, number, number, number]): number | undefined;
  rayCast(origin: Vec3, dir: Vec3, maxT: number, mask: number): RayHit | null | undefined;
  landHeight(x: number, z: number): number | undefined;
  fullHeight(p: Vec3): number | undefined;
  heightAttr(x: number, z: number): number | undefined;
  blinkPoint(self: Vec3, to: Vec3, range: number): BlinkResult | undefined;
  targetCheck(target: number, range: number, coneCos: number): number | undefined;
  rangePoint(id: number, p: Vec3, r: number): boolean | undefined;
  /** Cap C: a second request in the same iteration returns the previous result without a call. */
  cameraCheckBound(eye: Vec3, lookAt: Vec3): CameraBound;

  // ---- views and bookkeeping ---------------------------------------------------------------------------------
  /** The render view of the last fresh `frame`, `null` before the first one. Valid until the next wasm call. */
  readonly view: RenderViewReader | null;
  readonly events: SimEventRouter;
  readonly frameSkew: number;
  readonly stats: SimStats;
}

/** Camera result of one rendered frame (43 section 8.8, opaque to the loop except the listener data). */
export interface CameraSample {
  /** Listener position for 3-D audio: the self position (LH). */
  readonly listener: Vec3Tuple;
  /** Camera yaw (`curRotx`, degrees) for 3-D audio. */
  readonly yaw: number;
  /** The camera state handed to `render.draw` (41 `CameraState`). */
  readonly camera: unknown;
}

/**
 * The part of the targeting state (43 section 7) that the server-driven messages of the combat runtime reach: the focus follows
 * `SetTargetNtf` / `ClearTarget`, a death moves it on (`__TARGETSAVE`, next target), a spawn can restore the focus of a flight.
 */
export interface TargetingPort {
  /** The focused object (a mover, ground item or ctrl), `null` for none. */
  focus(): number | null;
  /** `SetTargetNtf`: focus the object when it is a live mover in view; anything else is ignored (43 section 7.3). */
  onServerTarget(id: number): void;
  /** `ClearTarget`: the server forces the client to drop its focus. */
  onClearTarget(): void;
  /** `MoverDeath` (the own part of 17 section 11.1 / 43 section 7.11). */
  onMoverDeath(victim: number, attacker: number): void;
  /** A mover spawned: while the own player flies, the object it had focused before reappears as the focus again. */
  onMoverSpawned(id: number): void;
}

/**
 * What the renderer and the HUD draw of the targeting (43 section 7.1, 7.7): the focus bracket, the target frame of the render
 * target (the hostile focus or the queued next target) and the charge gauge of a bow or wand. Valid until the next call.
 */
export interface TargetView {
  readonly focus: number | null;
  /** The object whose target frame is shown (`null` for none). */
  readonly renderTarget: number | null;
  /** The queued next target during an auto-attack. */
  readonly nextTarget: number | null;
  readonly autoAttack: boolean;
  /** `RenderGauge(0..4)` of the last world tick, `null` for no gauge. */
  readonly gauge: number | null;
  /** The fly targets of the last frame (ascending ObjId) while the own player flies (43 section 9.4); the renderer boxes them. */
  readonly flyTargets?: readonly number[];
}

/**
 * The part of the input layer (43 section 7.10, 7.12) that the skill runtime of 17 (`game/combat/skills.ts`) reaches: the answers of
 * the server to `UseSkillReq`, the end of the action-slot chain and the system lines of the skill code.
 */
export interface SkillInputPort {
  /** The own `UseSkillNtf` or `ClearUseSkill`: the request is answered (`REQ_USESKILL` off, `DPClient.cpp:4036, 4087`). */
  clearReqUseSkill(): void;
  /** `ClearUseSkill` of the own player: `REQ_USESKILL` off, the action slot stops (`m_nExecute = 0`) and the reservation ends. */
  onClearUseSkill(): void;
  /** `EndSkillQueueNtf`: the action-slot chain ended (`OnCancelSkill`). */
  onEndSkillQueue(): void;
  /** An own skill started: the world auto-attack begins when the conditions of ref/09a 5.3 hold. */
  onOwnSkillStarted(skillId: number, exeTarget: number): void;
  /** `SetNextSkill(NEXTSKILL_NONE)` (the Blinkpool line is blocked). */
  clearNextSkill(): void;
  /** A system line of a client text id (`TID_GAME_BLOCKTARGETING`). */
  notify(tid: number): void;
  /** `CWndMgr::UseSkillShortCut(0, slot)`: an F-key skill, a reserved skill or the skill window. */
  useSkillShortcut(slot: number): void;
  /** The action-slot shortcut / the C key (`CWndWorld::UseSkill`): the queue starts on the next world tick. */
  useActionSlot(): void;
}

/** 43: input and camera. */
export interface InputApi {
  /** The skill side of the input layer for the skill runtime; absent on the null port. */
  readonly skills?: SkillInputPort;
  /** The targeting state for the message handlers of 17; absent on the null port. */
  readonly targeting?: TargetingPort;
  /** The render-facing view of the targeting; absent on the null port. */
  targetView?(): TargetView;
  /** Sends pending immediate input (chat keys, hotkeys that act at once). */
  flushImmediate(): void;
  /** Releases all held keys and pointer capture (hidden tab, focus loss, modal windows). */
  release(): void;
  /**
   * Samples the world input once per iteration that steps frames; the record is passed to `sim.frame`
   * (held bits apply to all stepped frames, edges to the first, 43 section 2.2).
   */
  worldTick?(nowMs: number): unknown;
  /** Processes the stepped frames and returns the camera interpolated to `alpha` (rendered iterations only). */
  camera?(due: number, alpha: number, view: unknown): CameraSample | null;
  /** Mouse-over picking after the camera (rendered iterations only). */
  hover?(): void;
  /** `CCamera::SetQuake( ticks, size )` for the boss attacks (render-audio-m7); the callers gate it by `features.no_quake`. */
  quake?(ticks: number, size: number): void;
}

/** 44: audio engine. */
export interface AudioApi {
  /** Once per stepped 60 Hz frame, also in background pumps (BGM director, region tracker). */
  tick(): void;
  /** Once per rendered frame. */
  setListener(listener: Vec3Tuple, yaw: number): void;
  /** The cue functions of Plan/44 section 10.1 (`audio/cues.ts`); absent on the null port. */
  readonly cues?: AudioCues;
}

/** What 41 receives per rendered frame (section 10.2 step 10). */
export interface RenderFrame {
  readonly view: unknown;
  readonly camera: unknown;
  readonly nowMs: number;
  /** The interpolation factor that was used for this frame (`[0, 1]`). */
  readonly alpha: number;
}

/** The render side of the loading gate (Plan/40 section 9.8 step 5; `render/world/WorldScene.WorldGate` is a superset). */
export interface RenderGateState {
  /** The present lands of the inner 3 x 3 are resident. */
  readonly terrain: { readonly ready: number; readonly wanted: number };
  /** The static models of the viewer's own land are loaded. */
  readonly statics: { readonly ready: number; readonly wanted: number };
  /** Every render condition holds. */
  readonly open: boolean;
}

/** 41: world renderer (null until the lazily imported render chunk is ready). */
export interface RenderApi {
  /** Cosmetic 60 Hz systems (SFX, particles, damage numbers); `due` steps, skipped while hidden. */
  tick(due: number): void;
  draw(frame: RenderFrame): void;
  /** The world to show (`setWorld` of 41, Plan/40 section 9.8 step 2); `null` leaves the world (22.5). */
  setWorld?(world: WorldRenderDesc | null): void;
  /** The render side of the loading gate; `null` while no world is set. Absent = nothing to wait for. */
  worldGate?(): RenderGateState | null;
  /** The own mover's parts are loaded (the gate waits for the player model); absent = nothing to wait for. */
  ownModelReady?(objId: number): boolean;
  /**
   * Picking support of 41 for 43 (Plan/41 section 20, Plan/43 section 5): the objects drawn in the last frame, the precise
   * triangle test on the current pose (three.js-space ray), the screen rectangle of an object (CSS px) and the mouse-over
   * highlight (`0` clears). Absent before the renderer exists.
   */
  visibleObjects?(): readonly PickProxy[];
  raycastPrecise?(id: number, ray: Ray3): number | null;
  projectBounds?(id: number): ScreenRect | null;
  setHighlight?(id: number): void;
  /**
   * The effect requests of the feature handlers (`fx.balloon` / `emoticon` / `vendorTitle` / `damageNumber`, Plan/41 section 2.2):
   * the renderer's `fx` composed with the in-world overlay (Plan/41 19, WP-REND-14). Absent until the renderer exists, so a
   * handler reads it at the moment of the call (`ctx.render.fx?.balloon(...)`), never once at registration.
   */
  readonly fx?: RenderFx | undefined;
  /**
   * `DuelCount` (Plan/41 section 19.4, WP-REND-25): the PvP countdown sprite 3, 2, 1 (0 = the word "fight") in the middle of the view
   * for 1.5 s, drawn by the in-world overlay.
   */
  pvpCountdown?(n: number): void;
  /**
   * The overlay drew the countdown sprite in the last frame (the canvas sprite of `SFX/CountFight.dds` is resident): the DOM text of
   * the duel feature, which stands in until then, hides.
   */
  pvpCountdownDrawn?(): boolean;
  /** `XI_GEN_MOVEMARK01` at a click-to-move destination (Plan/43 section 6.6; LH position and hit-triangle normal). */
  moveMark?(
    pos: readonly [number, number, number],
    normal: readonly [number, number, number] | null,
  ): void;
  // ---- housing / guild house (ui-house-quiz)
  /** The placement ghost of a housing or guild-house furniture (`CDeployManager`, Plan/41 WP-REND-27): the controller of `features/events/deploy` drives it. Absent until the renderer exists. */
  readonly deploy?: DeployRenderApi | undefined;
}

// ---- housing / guild house (ui-house-quiz)
/** A triangle mesh in LEFT-HANDED object-local space (`positions` x, y, z triples; `index` triples of vertex numbers). */
export interface DeployMesh {
  readonly positions: ArrayLike<number>;
  readonly index: ArrayLike<number>;
}

/** An object of the scene: its LH world matrix (D3DX row-vector order, 16 numbers), the LH box of its model and its triangles. */
export interface DeployBody {
  readonly matrix: readonly number[];
  readonly bbox: { readonly min: Vec3LH; readonly max: Vec3LH };
  readonly mesh: DeployMesh;
}

/** What the collision test needs, read from the loaded models (`DeployRenderApi.scene`). */
export interface DeployScene {
  /** The ghost's model: its LH box and its triangles (the pose is the controller's). */
  readonly ghost: {
    readonly bbox: { readonly min: Vec3LH; readonly max: Vec3LH };
    readonly mesh: DeployMesh;
  };
  /** The room wall object (`obj_miniwall01.o3d`), `null` when the world has none. */
  readonly wall: DeployBody | null;
  /** The other ctrls of the layer (id, LH world matrix, LH model box). */
  readonly ctrls: readonly {
    readonly id: number;
    readonly matrix: readonly number[];
    readonly bbox: { readonly min: Vec3LH; readonly max: Vec3LH };
  }[];
}

/** The placement ghost (`render/objects/deployGhost.ts`): one translucent model that follows the pointer and turns red on a collision. */
export interface DeployRenderApi {
  /** Loads the ctrl model `ctrlId` into the ghost (hidden until the first `pose`); a second call replaces it. */
  show(ctrlId: number): void;
  /** `Scale( 1 ) * RotYawPitchRoll( -angle ) * Translate( pos )`; blend 80, or 110 and the `red.tga` texture while `collide`. */
  pose(pos: Vec3LH, angleDeg: number, collide: boolean): void;
  /** Removes the ghost. */
  hide(): void;
  /** The scene for the collision test; `null` while the ghost model is loading. */
  scene(wallFile: string, exceptObjId: number): DeployScene | null;
  /** The world has the room wall object `file` (a guild placement refuses to start without it). */
  wallPresent(file: string): boolean;
}

/** 42: UI framework (null until `createUi`). */
export interface UiApi {
  /** Window fades, tooltips, the per-frame UI process; `dtMs` since the previous rendered frame. */
  process(dtMs: number): void;
  /** The dialog windows are loaded (`ui/wnd/lazy.ts`): the loading gate of the world waits for it; absent = ready. */
  windowsReady?(): boolean;
}

/** GPU upload queue (WP-CLIENT-22): runs the per-frame upload budget before the draw. */
export interface UploadsApi {
  runFrame(): void;
}

/** Move reporter (WP-CLIENT-20): turns the render view's report area into `PlayerMoveState` messages. */
export interface MoveReporterApi {
  afterStep(view: unknown): void;
}

/** Pose feeder (WP-CLIENT-18): requests the skeletons and clips the render view asks for. */
export interface PoseFeederApi {
  scan(view: unknown): void;
  /** The `skel_id` of a `.chr` path (the first call fetches the file); the wing of a ride (WP-REND-25) uses it. */
  skeletonId?(path: string): number;
  /** Makes sure the sim gets the clip `clip` of the motion table (the `.ani` is fetched once); decor entries use it. */
  requestClip?(clip: number): void;
}

/** Everything the loop calls, resolved once per iteration (so late-installed render/ui are picked up). */
export interface LoopPorts {
  readonly sim: SimBridge;
  readonly input: InputApi;
  readonly audio: AudioApi;
  readonly render: RenderApi | null;
  readonly ui: UiApi | null;
  readonly uploads: UploadsApi | null;
  readonly moveReporter: MoveReporterApi | null;
  readonly poseFeeder: PoseFeederApi | null;
}

export const NULL_INPUT: InputApi = {
  flushImmediate: () => undefined,
  release: () => undefined,
};

export const NULL_AUDIO: AudioApi = {
  tick: () => undefined,
  setListener: () => undefined,
};

/** A sim that steps every requested frame and produces nothing (no world before the bridge exists). */
export const NULL_SIM: SimBridge = new NullSim();

export const NULL_LOOP_PORTS: LoopPorts = {
  sim: NULL_SIM,
  input: NULL_INPUT,
  audio: NULL_AUDIO,
  render: null,
  ui: null,
  uploads: null,
  moveReporter: null,
  poseFeeder: null,
};
