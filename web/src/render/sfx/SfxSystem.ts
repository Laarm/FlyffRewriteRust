// The SFX runtime (Plan/41 sections 16.6, 17.1; `CSfx` + `CSfxModel`, SfxObj.cpp, World3D.cpp:636-700, 1179-1230): the effect
// instances of the world, their 60 Hz `process()` steps, the per-frame placement, culling, ordering and the batched draw.
//
//   create / remove      `RenderFx.sfx` / `removeSfx` / `markRemoveSfx`: an instance per request, a handle per instance
//   tick(steps)          `CSfx::Process`: frame counters, `nSec` semantics, the model's `process()`
//   update(frame)        follow the source / destination object (render-view positions), cull, sort far to near, fill the draw
//                        list (`emitModel`), hand it to the GL renderer
//
// An instance whose `.sfx` is still loading does nothing; one whose first textures are loading is held back for at most
// `MAX_HOLD_TICKS` so that a cold effect does not lose its first frames; a missing file is invisible (KQ-30).
//
// Culling (PL-123): the bounding sphere of the prototype (scaled by the instance; a MESH part reaches as far as its loaded model,
// `MESH_EXTENT_FALLBACK_M` while it loads, #018 B9) against the frustum, the `fDistant` row of the effect's `sfx-table` class
// against the camera distance to the SURFACE of that sphere, and the options of ref/20 section 14 - `sfxLevel <= 0` hides the
// non-skill effects of other players, `sfxRenderOff` the skill effects of other players and every effect whose render flag is
// not 1. Effects are drawn far to near by the distance of their position; when the quad budget (50 000) cannot hold them all the
// FARTHEST ones lose their particles first (`stats.sfxDropped`).

import { Sphere, Vector3, type Frustum } from 'three';
import { Prio } from '../../workers/priority';
import type { SfxClass, SfxHandle, SfxOptions } from '../fx';
import { cullDistance } from '../objects/distanceTable';
import type { RenderOptions } from '../quality';
import { BASE_SPEC, REMOVABLE_XI, fixedAngle, isFixedHit, specOf, type ClassSpec } from './classes';
import type { BehaviorHost, SkillFxHooks } from './classSpec';
import { preCreate } from './skillClasses';
import { DrawList, MAX_QUADS } from './DrawList';
import { type Aff, fromMat16, newAff } from './mat';
import { directionRot3, surfaceRot3 } from './orient';
import { type EmitEnv, emitModel } from './parts';
import { meshPartRadius, meshPartsOf, type SfxProto } from './proto';
import { type Randf, randomRandf } from './rng';
import { SfxModel } from './SfxModel';
import type { SfxPrototypes } from './SfxPrototypes';
import type { SfxRenderer } from './SfxRenderer';
import type { SfxTextures } from './SfxTextures';

/** Frames an effect waits for its first textures (0.5 s). */
export const MAX_HOLD_TICKS = 30;

/**
 * The reach assumed for the model of a MESH part until it has loaded (`SfxMeshes.extentOf` answers `undefined`): generous, so an
 * effect whose pivot is just off screen is not culled before its model is known (#018 B9). 8 m covers the world effects with
 * mesh parts (the gate, the lodestars, the Xmas tree reach 2 to 14 m).
 */
export const MESH_EXTENT_FALLBACK_M = 8;
/** Live instances above which new requests are refused. */
export const MAX_INSTANCES = 1500;
/** Where an effect request waits for the render view to know its anchor before it is dropped (ticks). */
export const MAX_UNPOSITIONED_TICKS = 600;

export type V3 = [number, number, number];

/** The `sfx-table` row an instance needs. */
export interface SfxCatalogRow {
  /** Canonical `.sfx` path, `null` when the file does not exist. */
  readonly file: string | null;
  readonly scale: number;
  /** `MD_*` view-distance class. */
  readonly distant: number;
  readonly renderFlag: number;
}

export interface SfxCatalog {
  row(xi: number): SfxCatalogRow | undefined;
}

/** What the system asks the world (the render view and the entity table of the frame). */
export interface WorldQuery {
  /** The logical position (LH) of an object, `false` when the object does not exist. */
  posOf(id: number, out: V3): boolean;
  /** The object's yaw in degrees, `NaN` when unknown. */
  angleOf(id: number): number;
  /** The object is a player (not a monster or NPC). */
  isPlayer(id: number): boolean;
  /** The `OBJSTA_*` state word of a mover (`0` when unknown). */
  stateOf?(id: number): number | undefined;
  /** `CModelObject::GetHandPos`: the right (0) / left (1) hand of a mover in the world (LH); `false` without a record. */
  handPos?(id: number, hand: 0 | 1, out: V3): boolean;
  /** `CModelObject::GetEventPos` + the world matrix: event point `index` of a mover (LH); `false` without a record. */
  eventPos?(id: number, index: number, out: V3): boolean;
  /** `CWorld::GetLandHeight_Fast(x, z)`. */
  heightAt?(x: number, z: number): number | undefined;
  /** `CMover::GetRadius()`. */
  radiusOf?(id: number): number | undefined;
  /** `GetMaxHeight() * s - 0.7 * s` of a mover (`s` = its y scale): the height of the troupe blitz mark above its feet. */
  headOf?(id: number): number | undefined;
  // ---- render-audio-m7 (WP-REND-27, doc 41 section 12.4)
  /** `world * bone[b] * (localLH)` of a mover: a point in the frame of bone `bone`, in the world (LH); `false` without the pose. */
  bonePos?(id: number, bone: number, localLH: readonly [number, number, number], out: V3): boolean;
  /** The mover's world matrix and bone `bone`'s matrix as LEFT-HANDED D3D row-major matrices (16 numbers each) into `world` / `bone`; `false` without the pose. */
  bonePose?(id: number, bone: number, world: number[], bone16: number[]): boolean;
  /** `MoverProp.dwClass` of a mover (`RANK_*`). */
  rankOf?(id: number): number | undefined;
  /** `GetScale().x` of a mover: the length of the first axis of its world matrix. */
  scaleOf?(id: number): number | undefined;
}

/** The world with nothing in it. */
export const EMPTY_WORLD: WorldQuery = {
  posOf: () => false,
  angleOf: () => Number.NaN,
  isPlayer: () => false,
};

/** One frame of the system. */
export interface SfxFrame {
  /** The camera position (LH). */
  readonly cameraPos: readonly [number, number, number];
  /** The view frustum in three.js space. */
  readonly frustum: Frustum;
  /** `g_matInvView` (LH, 16 numbers, translation zeroed). */
  readonly invViewRot: ArrayLike<number>;
  readonly world: WorldQuery;
}

export interface SfxSystemDeps {
  readonly prototypes: SfxPrototypes;
  readonly textures: SfxTextures;
  readonly catalog: SfxCatalog;
  readonly renderer: Pick<SfxRenderer, 'render' | 'stats'>;
  /**
   * The MESH part handler: told about every new model so that the part models and their textures load before the first draw, and
   * asked how far a loaded model reaches from its pivot (`SfxMeshes.extentOf`; `undefined` while it is loading) for the culling.
   */
  readonly meshes?: {
    prepare(model: SfxModel): void;
    extentOf?(path: string): number | undefined;
  };
  readonly options: () => Pick<RenderOptions, 'sfxLevel' | 'sfxRenderOff' | 'objectDistant'>;
  readonly randf?: Randf;
  /** `xRandomF(1)` for the skill classes (rain jitter, drop speed): uniform in [0, 1]; default `Math.random`. */
  readonly uniform?: () => number;
  /** The game's answers for the skill classes (party skill effects, duel state, ...). */
  readonly hooks?: SkillFxHooks;
  /** Extra parts of the frame drawn into the same list after the instances (the boss laser and fire breath, render-audio-m7): `textureId` is `SfxTextures.drawable`. */
  readonly extras?: {
    draw(list: DrawList, frame: SfxFrame, textureId: (path: string) => number): void;
  };
  /** The quad budget (default {@link MAX_QUADS}). */
  readonly maxQuads?: number;
  readonly log?: (message: string) => void;
}

export interface SfxStats {
  /** Live instances. */
  instances: number;
  /** Instances drawn in the last frame. */
  drawn: number;
  culled: number;
  quads: number;
  dropped: number;
  runs: number;
  draws: number;
  /** Requests refused (unknown file, cap). */
  rejected: number;
}

/** One live effect (`CSfx`). */
export class SfxInstance {
  dead = false;
  model: SfxModel | null = null;
  proto: SfxProto | null = null;
  /** `m_nFrame`. */
  frame = 0;
  hold = 0;
  /** The first update with a known position has run. */
  positioned: boolean;
  unpositioned = 0;
  /** The angle comes from the source's yaw at the first update. */
  angleFromSrc: boolean;
  readonly pos: V3 = [0, 0, 0];
  readonly posDest: V3 = [0, 0, 0];
  /** `m_fAngle`: the model yaw. */
  angle = 0;
  /** The 3-axis rotation of a directional effect (degrees `x y z`), `null` = yaw-only. */
  rot3: V3 | null = null;
  readonly scale: V3 = [1, 1, 1];
  idSrc = 0;
  idDest = 0;
  nSec = 0;
  isStatic = false;
  staticLand = -1;
  /** A projectile visual: the projectile code places it every frame (`placeProjectile`). */
  driven = false;
  /** The in-flight frame loop of a projectile class: when `cur` reaches `at` it jumps to `to` (`ge`: at or past). */
  loop: FrameLoop | null = null;
  /** Past positions drawn as afterimages (`CSfxShootWave`), `ghostCount` triples of LH numbers. */
  ghosts: Float64Array | null = null;
  ghostCount = 0;
  /** `CSfxPartsLink::m_nPartsLink`: 0 = the right hand, 1 = the left hand. */
  partLink: 0 | 1 = 0;
  /** `CSfxDuelParty::m_nType` (`ENEMY_*`). */
  duelKind = 0;
  /** The position at creation (`SetPos` of the factory, lifted by 0.01): the skill classes place themselves relative to it. */
  readonly origin: V3 = [0, 0, 0];
  /** The model finished and the class keeps the instance (`afterModel` said `spent`): it is no longer drawn. */
  spent = false;
  /** The model part 0 draws instead of its own. */
  meshPath: string | null = null;
  /** `CSfxAtkStraight`: the step per tick (`ShootSfx`), the explosion effect and whether it was shot (render-audio-m7). */
  readonly delta: V3 = [0, 0, 0];
  explosion = 0;
  shot = false;
  /** The class state (`skillBehaviors.ts`). */
  readonly bh: BehaviorState = { count: 0, timer: 0, child: 0, armed: false, seen: 0, flag: false };

  constructor(
    readonly handle: number,
    readonly xi: number,
    readonly cls: SfxClass,
    readonly spec: ClassSpec,
    readonly file: string,
    readonly distant: number,
    readonly renderFlag: number,
    positioned: boolean,
    angleFromSrc: boolean,
  ) {
    this.positioned = positioned;
    this.angleFromSrc = angleFromSrc;
  }
}

/** The scratch state of a behaviour class. */
export interface BehaviorState {
  count: number;
  timer: number;
  child: number;
  armed: boolean;
  seen: number;
  flag: boolean;
}

/** The frame loop of a projectile class (`if (cur == 20) cur = 0`, `if (cur == 60) cur = 30`, ...). */
export interface FrameLoop {
  readonly at: number;
  readonly to: number;
  readonly ge?: boolean;
}

/** A projectile visual request (Plan/41 17.4). */
export interface ProjectileSfx {
  /** The start position (LH). */
  readonly pos: readonly [number, number, number];
  /** The 3-axis rotation of a directional class (degrees `x y z`), `null` for the yaw-only path. */
  readonly rot3: readonly [number, number, number] | null;
  /** The model yaw of the yaw-only path (degrees). */
  readonly yaw?: number;
  readonly scale?: number;
  readonly loop?: FrameLoop;
  readonly cls?: SfxClass;
  /** Plays once and ends with its model (the default: loops until removed). */
  readonly once?: boolean;
  /** The model part 0 draws instead of its own (`__YOYO_MODEL_FIX`). */
  readonly meshPath?: string;
}

/** The projectile classes draw at the flight position itself (no lift) and loop their animation until they are removed. */
const PROJECTILE_SPEC: ClassSpec = {
  follow: 'none',
  lift: 0,
  yaw: true,
  render2: false,
  once: false,
  singleton: false,
  removable: false,
};

/** One placed `MODELTYPE_SFX` object of a land (Plan/41 11.1). */
export interface StaticSfx {
  readonly file: string;
  readonly distant: number;
  readonly renderFlag: number;
  readonly pos: readonly [number, number, number];
  readonly angle: number;
  readonly scale: readonly [number, number, number];
}

const MARK_TTL_TICKS = 600;

export class SfxSystem {
  readonly list: DrawList;
  readonly stats: SfxStats = {
    instances: 0,
    drawn: 0,
    culled: 0,
    quads: 0,
    dropped: 0,
    runs: 0,
    draws: 0,
    rejected: 0,
  };
  private readonly instances: SfxInstance[] = [];
  private readonly byHandle = new Map<number, SfxInstance>();
  private readonly marks = new Map<string, number>();
  private readonly firstTextures = new WeakMap<SfxProto, readonly string[]>();
  private nextHandle = 1;
  private tickNow = 0;
  private moveMark: SfxInstance | null = null;
  private readonly randf: Randf;
  private readonly invView: Aff = newAff();
  private readonly tmp: V3 = [0, 0, 0];
  private disposed = false;
  // scratch for the frame
  private order: SfxInstance[] = [];
  private keys: number[] = [];
  private readonly env: EmitEnv;
  /** What the behaviours read through their host: the world of the last frame and the game's hooks. */
  private readonly shared: { world: WorldQuery; hooks: SkillFxHooks } = {
    world: EMPTY_WORLD,
    hooks: {},
  };
  private readonly uniform: () => number;
  private readonly host: BehaviorHost;

  constructor(private readonly deps: SfxSystemDeps) {
    this.randf = deps.randf ?? randomRandf();
    this.shared.hooks = deps.hooks ?? {};
    this.uniform = deps.uniform ?? Math.random;
    const shared = this.shared;
    this.host = {
      get world() {
        return shared.world;
      },
      get hooks() {
        return shared.hooks;
      },
      create: (xi, o) => this.create(xi, o),
      alive: (h) => this.instanceOf(h) !== undefined,
      rand: () => this.uniform(),
    };
    this.list = new DrawList(deps.maxQuads ?? MAX_QUADS);
    this.env = {
      invViewRot: this.invView,
      textureId: (path) => deps.textures.drawable(path),
    };
  }

  // ---- requests ----------------------------------------------------------------------------------------------------

  /** `CreateSfx`: a new effect. Returns its handle (also for requests that cannot be shown: removing it is harmless). */
  create(xi: number, o: SfxOptions = {}): SfxHandle {
    const handle = this.nextHandle++;
    if (this.disposed) return handle;
    const row = this.deps.catalog.row(xi);
    const file = row?.file ?? null;
    if (row === undefined || file === null) {
      this.stats.rejected++;
      return handle;
    }
    if (this.instances.length >= MAX_INSTANCES) {
      this.stats.rejected++;
      return handle;
    }
    // the factory's post-creation fixes (CreateObj.cpp:263-340): a cleared destination, an angle toward the target, a scale
    const fixed = preCreate(xi, o, this.world);
    o = fixed.o;
    const spec = specOf(xi);
    const src = o.src ?? 0;
    const dest = o.dest ?? 0;
    const hasPos = o.pos !== undefined;
    const inst = new SfxInstance(
      handle,
      xi,
      o.cls ?? 'normal',
      spec,
      file,
      row.distant,
      row.renderFlag,
      hasPos || (src === 0 && dest === 0),
      o.angle === undefined && isFixedHit(xi) && src !== 0,
    );
    inst.idSrc = src;
    inst.idDest = dest;
    inst.nSec = o.nSec ?? 0;
    inst.partLink = o.partLink ?? 0;
    inst.duelKind = o.duelKind ?? 0;
    if (o.pos) {
      inst.pos[0] = o.pos[0];
      inst.pos[1] = o.pos[1];
      inst.pos[2] = o.pos[2];
    }
    // `SetSfx`: the position is lifted by 0.01 so that a flat effect does not z-fight with the ground
    inst.pos[1] += 0.01;
    inst.origin[0] = inst.pos[0];
    inst.origin[1] = inst.pos[1];
    inst.origin[2] = inst.pos[2];
    if (o.posDest) {
      inst.posDest[0] = o.posDest[0];
      inst.posDest[1] = o.posDest[1];
      inst.posDest[2] = o.posDest[2];
    } else {
      inst.posDest[0] = inst.pos[0];
      inst.posDest[1] = inst.pos[1];
      inst.posDest[2] = inst.pos[2];
    }
    const s = o.scale ?? row.scale;
    inst.scale[0] = s;
    inst.scale[1] = s;
    inst.scale[2] = s;
    if (o.angle !== undefined) inst.angle = o.angle;
    if (o.rotateDeg) inst.rot3 = [o.rotateDeg[0], o.rotateDeg[1], o.rotateDeg[2]];
    if (spec.render2 && inst.rot3 === null) inst.rot3 = [0, 0, 0];
    if (o.directional && o.posDest) inst.rot3 = directionRot3(inst.pos, o.posDest);
    if (spec.singleton) this.replaceSingleton(inst);
    spec.behavior?.init?.(inst, this.host, o);
    this.addInstance(inst);
    // `XI_NAT_WASTART01` / `XI_NAT_CUPITSTART01` also start the wings with the same parameters
    for (const [childXi, childO] of fixed.also) this.create(childXi, childO);
    return handle;
  }

  private addInstance(inst: SfxInstance): void {
    this.instances.push(inst);
    this.byHandle.set(inst.handle, inst);
    if (inst.spec.behavior?.modelless === true) return; // the rain spawners draw nothing: no file to wait for
    // the prototype and its textures start loading at once
    const proto = this.deps.prototypes.get(inst.file, Prio.Fx);
    if (proto) this.attach(inst, proto);
  }

  private replaceSingleton(inst: SfxInstance): void {
    const old = this.moveMark;
    // a new click fast-forwards the old mark to frame 180 (WndWorldControlPlayer.cpp:345)
    if (old && !old.dead && old.model) old.model.cur = 180;
    this.moveMark = inst;
  }

  /** `RenderFx.removeSfx`. */
  remove(handle: SfxHandle): void {
    const inst = this.byHandle.get(handle);
    if (inst) inst.dead = true;
  }

  /**
   * A projectile visual (`CSfxShoot` and its relatives): the effect plays its animation in a loop at the position the projectile code
   * gives it (`placeProjectile`), through the 3-axis path (`Render2`) when `rot3` is given, else yaw-only, until `remove`.
   */
  createProjectile(xi: number, o: ProjectileSfx): SfxHandle {
    const handle = this.nextHandle++;
    if (this.disposed) return handle;
    const row = this.deps.catalog.row(xi);
    const file = row?.file ?? null;
    if (row === undefined || file === null || this.instances.length >= MAX_INSTANCES) {
      this.stats.rejected++;
      return handle;
    }
    const spec: ClassSpec = { ...PROJECTILE_SPEC, render2: o.rot3 !== null };
    const inst = new SfxInstance(
      handle,
      xi,
      o.cls ?? 'skill',
      spec,
      file,
      row.distant,
      row.renderFlag,
      true,
      false,
    );
    inst.driven = true;
    inst.nSec = o.once === true ? 0 : -1;
    inst.meshPath = o.meshPath ?? null;
    inst.loop = o.loop ?? null;
    const s = o.scale ?? row.scale;
    inst.scale[0] = s;
    inst.scale[1] = s;
    inst.scale[2] = s;
    this.placeInstance(inst, o.pos, o.rot3, o.yaw ?? 0);
    inst.posDest[0] = inst.pos[0];
    inst.posDest[1] = inst.pos[1];
    inst.posDest[2] = inst.pos[2];
    this.addInstance(inst);
    return handle;
  }

  /** Moves a projectile visual (every frame, from the render view record). */
  placeProjectile(
    handle: SfxHandle,
    pos: readonly [number, number, number],
    rot3: readonly [number, number, number] | null,
    yaw = 0,
  ): void {
    const inst = this.byHandle.get(handle);
    if (inst?.driven === true && !inst.dead) this.placeInstance(inst, pos, rot3, yaw);
  }

  private placeInstance(
    inst: SfxInstance,
    pos: readonly [number, number, number],
    rot3: readonly [number, number, number] | null,
    yaw: number,
  ): void {
    inst.pos[0] = pos[0];
    inst.pos[1] = pos[1];
    inst.pos[2] = pos[2];
    if (rot3) {
      if (inst.rot3) {
        inst.rot3[0] = rot3[0];
        inst.rot3[1] = rot3[1];
        inst.rot3[2] = rot3[2];
      } else {
        inst.rot3 = [rot3[0], rot3[1], rot3[2]];
      }
    }
    inst.angle = yaw;
  }

  /** The afterimages of a visual (`CSfxShootWave`): the model is drawn once more at each position, oldest last. */
  setGhosts(handle: SfxHandle, positions: readonly (readonly [number, number, number])[]): void {
    const inst = this.byHandle.get(handle);
    if (!inst || inst.dead) return;
    const n = positions.length;
    if (!inst.ghosts || inst.ghosts.length < n * 3)
      inst.ghosts = new Float64Array(Math.max(24, n * 3));
    for (let i = 0; i < n; i++) {
      const p = positions[i];
      if (!p) continue;
      inst.ghosts[i * 3] = p[0];
      inst.ghosts[i * 3 + 1] = p[1];
      inst.ghosts[i * 3 + 2] = p[2];
    }
    inst.ghostCount = n;
  }

  /**
   * `RemoveSfxObj` of the server (`OnRemoveSfxObj`): a looping effect on `anchor` ends. The effects that read the mover's remove
   * mark (`REMOVABLE_XI`) consume a one-shot mark when their next step runs; any other matching effect ends at once.
   */
  markRemove(anchor: number, xi: number): void {
    if (REMOVABLE_XI.has(xi)) {
      this.marks.set(`${String(anchor)}:${String(xi)}`, this.tickNow + MARK_TTL_TICKS);
      return;
    }
    for (const inst of this.instances) {
      if (inst.xi === xi && (inst.idDest === anchor || inst.idSrc === anchor)) inst.dead = true;
    }
  }

  /** The click-to-move mark (`XI_GEN_MOVEMARK01`, singleton) lying on the ground with surface normal `normal`. */
  placeMoveMark(
    xi: number,
    pos: readonly [number, number, number],
    normal: readonly [number, number, number] | null,
  ): SfxHandle {
    const rot = normal ? surfaceRot3(normal) : undefined;
    return this.create(xi, { pos, ...(rot ? { rotateDeg: rot } : {}), cls: 'ui' });
  }

  /** The effect instance of a handle (tests, probes). */
  instanceOf(handle: SfxHandle): SfxInstance | undefined {
    const inst = this.byHandle.get(handle);
    return inst && !inst.dead ? inst : undefined;
  }

  get live(): readonly SfxInstance[] {
    return this.instances;
  }

  // ---- static effects (`MODELTYPE_SFX` objects of a land) -----------------------------------------------------------

  /** The static effects of a land that became resident; they play forever (frame reset on finish). */
  addStaticLand(land: number, items: readonly StaticSfx[]): void {
    this.removeStaticLand(land);
    for (const it of items) {
      const inst = new SfxInstance(
        this.nextHandle++,
        0,
        'normal',
        { ...BASE_SPEC, follow: 'none', lift: 0.2 },
        it.file,
        it.distant,
        it.renderFlag,
        true,
        false,
      );
      inst.isStatic = true;
      inst.staticLand = land;
      inst.nSec = -1;
      inst.pos[0] = it.pos[0];
      inst.pos[1] = it.pos[1];
      inst.pos[2] = it.pos[2];
      inst.angle = it.angle;
      inst.scale[0] = it.scale[0];
      inst.scale[1] = it.scale[1];
      inst.scale[2] = it.scale[2];
      this.addInstance(inst);
    }
  }

  removeStaticLand(land: number): void {
    for (const inst of this.instances)
      if (inst.isStatic && inst.staticLand === land) inst.dead = true;
  }

  // ---- the model ---------------------------------------------------------------------------------------------------

  private attach(inst: SfxInstance, proto: SfxProto): void {
    inst.proto = proto;
    const model = new SfxModel(proto, this.randf);
    inst.model = model;
    model.meshOverride = inst.meshPath;
    model.scale[0] = inst.scale[0];
    model.scale[1] = inst.scale[1];
    model.scale[2] = inst.scale[2];
    // request every texture of the first frames now; the rest follow when a part first draws
    for (const path of this.firstTexturesOf(proto)) this.deps.textures.idOf(path);
    this.deps.meshes?.prepare(model);
  }

  private firstTexturesOf(proto: SfxProto): readonly string[] {
    let list = this.firstTextures.get(proto);
    if (!list) {
      const set = new Set<string>();
      for (const p of proto.parts) {
        if (!p.inUse || p.kind === 'mesh') continue;
        const first = p.texturePaths[0];
        if (first !== undefined) set.add(first);
      }
      list = [...set];
      this.firstTextures.set(proto, list);
    }
    return list;
  }

  private texturesReady(proto: SfxProto): boolean {
    for (const path of this.firstTexturesOf(proto)) {
      if (this.deps.textures.stateOf(path) === 'loading') return false;
    }
    return true;
  }

  // ---- the 60 Hz step ----------------------------------------------------------------------------------------------

  /** `steps` cosmetic ticks: every instance runs `CSfx::Process`. */
  tick(steps: number): void {
    if (this.disposed) return;
    for (let s = 0; s < steps; s++) {
      this.tickNow++;
      for (const inst of this.instances) {
        if (!inst.dead) this.process(inst);
      }
    }
    this.sweep();
    if (this.marks.size > 0) {
      for (const [k, expiry] of this.marks) if (expiry <= this.tickNow) this.marks.delete(k);
    }
  }

  /** Attaches the model as soon as the prototype is there; marks a missing file dead. Returns `true` when the model exists. */
  private ensureModel(inst: SfxInstance): boolean {
    if (inst.model) return true;
    const proto = this.deps.prototypes.get(inst.file, Prio.Fx);
    if (proto) {
      this.attach(inst, proto);
      return true;
    }
    if (this.deps.prototypes.state(inst.file) === 'missing') inst.dead = true; // KQ-30: a missing effect is invisible
    return false;
  }

  private process(inst: SfxInstance): void {
    const bh = inst.spec.behavior;
    if (bh?.modelless === true) {
      if (bh.tick?.(inst, this.host) === 'end') inst.dead = true;
      inst.frame++;
      return;
    }
    if (!this.ensureModel(inst)) {
      if (!inst.dead && !inst.positioned && ++inst.unpositioned > MAX_UNPOSITIONED_TICKS)
        inst.dead = true;
      return;
    }
    const model = inst.model;
    const proto = inst.proto;
    if (!model || !proto) return;
    if (!inst.positioned) {
      if (++inst.unpositioned > MAX_UNPOSITIONED_TICKS) inst.dead = true; // the anchor never appeared
      return;
    }
    if (inst.hold < MAX_HOLD_TICKS && !this.texturesReady(proto)) {
      inst.hold++;
      return;
    }
    if (inst.spec.removable && inst.frame > 0 && this.consumeMark(inst)) {
      inst.dead = true;
      return;
    }
    if (bh?.tick?.(inst, this.host) === 'end') {
      inst.dead = true;
      return;
    }
    inst.frame++;
    const finished = model.process();
    if (
      inst.loop &&
      (inst.loop.ge === true ? model.cur >= inst.loop.at : model.cur === inst.loop.at)
    ) {
      model.cur = inst.loop.to;
    }
    if (bh?.ownLifetime === true) {
      // the class ends itself (`tick`); the model's end is its answer to give
      const verdict = bh.afterModel?.(inst, this.host, finished);
      if (verdict === 'end') inst.dead = true;
      else if (verdict === 'restart') model.restart();
      else if (verdict === 'spent') inst.spent = true;
      return;
    }
    if (inst.isStatic) {
      if (finished) model.restart();
    } else if (inst.spec.once || inst.nSec === 0) {
      if (finished) {
        inst.dead = true;
        bh?.finish?.(inst, this.host);
      }
    } else {
      if (finished) model.restart();
      if (inst.nSec !== -1 && inst.frame > inst.nSec * 60) inst.dead = true;
    }
  }

  /** Consumes the `RemoveSfxObj` mark of `anchor` for effect `xi` (`GetRemoveSfxObj`): `true` if there was one. */
  takeMark(anchor: number, xi: number): boolean {
    const key = `${String(anchor)}:${String(xi)}`;
    return this.marks.delete(key);
  }

  private consumeMark(inst: SfxInstance): boolean {
    const key = `${String(inst.idDest)}:${String(inst.xi)}`;
    if (inst.idDest === 0 || !this.marks.has(key)) return false;
    this.marks.delete(key);
    return true;
  }

  private sweep(): void {
    let w = 0;
    for (const inst of this.instances) {
      if (inst.dead) {
        this.byHandle.delete(inst.handle);
        if (this.moveMark === inst) this.moveMark = null;
      } else {
        this.instances[w++] = inst;
      }
    }
    this.instances.length = w;
  }

  // ---- the frame ---------------------------------------------------------------------------------------------------

  /** Places, culls, orders and draws the effects of the frame. */
  update(frame: SfxFrame): void {
    if (this.disposed) return;
    this.world = frame.world;
    fromMat16(this.invView, frame.invViewRot);
    const cam = frame.cameraPos;
    const opts = this.deps.options();
    const stats = this.stats;
    stats.drawn = 0;
    stats.culled = 0;
    stats.dropped = 0;
    const order = this.order;
    const keys = this.keys;
    order.length = 0;
    keys.length = 0;
    for (const inst of this.instances) {
      if (inst.dead) continue;
      const behavior = inst.spec.behavior;
      if (behavior?.modelless === true) continue;
      this.ensureModel(inst);
      if (!this.follow(inst, frame.world)) continue;
      const model = inst.model;
      const proto = inst.proto;
      if (!model || !proto || !inst.positioned || inst.spent) continue;
      if (behavior?.place?.(inst, this.host) === false) continue;
      if (!this.visible(inst, frame, opts)) {
        stats.culled++;
        continue;
      }
      order.push(inst);
      const dx = inst.pos[0] - cam[0];
      const dy = inst.pos[1] - cam[1];
      const dz = inst.pos[2] - cam[2];
      keys.push(dx * dx + dy * dy + dz * dz);
    }
    // far to near (`ObjSortFarToNear`): an index sort keeps the instance list untouched
    const idx = order.map((_, i) => i);
    idx.sort((a, b) => (keys[b] ?? 0) - (keys[a] ?? 0));
    // the quad budget: the nearest effects keep their quads, the farthest lose them
    const list = this.list;
    list.reset();
    let budget = list.maxQuads;
    const skip = new Set<number>();
    for (let k = idx.length - 1; k >= 0; k--) {
      const inst = order[idx[k] ?? 0];
      if (!inst?.model) continue;
      const est = (inst.model.liveParticles + (inst.proto?.quadParts ?? 0)) * (1 + inst.ghostCount);
      if (est > budget) {
        skip.add(idx[k] ?? 0);
        stats.dropped += est;
      } else {
        budget -= est;
      }
    }
    for (const i of idx) {
      if (skip.has(i)) continue;
      const inst = order[i];
      const model = inst?.model;
      if (!inst || !model) continue;
      const lift = inst.spec.lift;
      model.pos[0] = inst.pos[0];
      model.pos[1] = inst.pos[1] + lift;
      model.pos[2] = inst.pos[2];
      model.rotate[1] = inst.spec.yaw ? inst.angle : 0;
      model.scale[0] = inst.scale[0];
      model.scale[1] = inst.scale[1];
      model.scale[2] = inst.scale[2];
      const rot3 = inst.spec.render2 ? (inst.rot3 ?? [0, 0, 0]) : inst.rot3;
      const ghosts = inst.ghosts;
      if (ghosts && inst.ghostCount > 0) {
        // the afterimages first (older), the model itself on top (`CSfxShootWave::Render`)
        for (let g = inst.ghostCount - 1; g >= 0; g--) {
          model.pos[0] = ghosts[g * 3] ?? 0;
          model.pos[1] = (ghosts[g * 3 + 1] ?? 0) + lift;
          model.pos[2] = ghosts[g * 3 + 2] ?? 0;
          emitModel(model, this.env, list, rot3);
        }
        model.pos[0] = inst.pos[0];
        model.pos[1] = inst.pos[1] + lift;
        model.pos[2] = inst.pos[2];
      }
      emitModel(model, this.env, list, rot3);
      stats.drawn++;
    }
    // ---- render-audio-m7: the per-boss laser and fire breath, once per frame, behind the instances
    this.deps.extras?.draw(list, frame, (path) => this.env.textureId(path));
    stats.quads = list.quads;
    stats.dropped += list.dropped;
    stats.instances = this.instances.length;
    this.deps.renderer.render(list);
    stats.runs = this.deps.renderer.stats.quadRuns + this.deps.renderer.stats.stripRuns;
    stats.draws = this.deps.renderer.stats.draws;
  }

  /** `CSfx::Process` position rules (SfxObj.cpp:176-232). Returns `false` when the instance died. */
  private follow(inst: SfxInstance, world: WorldQuery): boolean {
    const tmp = this.tmp;
    if (inst.angleFromSrc && inst.idSrc !== 0) {
      const a = world.angleOf(inst.idSrc);
      if (!Number.isNaN(a)) {
        const fixed = fixedAngle(inst.xi, a);
        if (fixed !== null) inst.angle = fixed;
        inst.angleFromSrc = false;
      }
    }
    switch (inst.spec.follow) {
      case 'base':
        if (inst.idDest !== 0) {
          if (world.posOf(inst.idDest, tmp)) {
            copy3(inst.posDest, tmp);
            copy3(inst.pos, inst.posDest);
            inst.positioned = true;
          } else if (inst.positioned) {
            copy3(inst.pos, inst.posDest); // the destination is gone: it keeps the last position
          }
        } else if (inst.idSrc !== 0) {
          if (world.posOf(inst.idSrc, tmp)) {
            copy3(inst.pos, tmp);
            inst.positioned = true;
          } else if (inst.positioned) {
            inst.dead = true; // the source is gone
            return false;
          }
        }
        break;
      case 'src':
        if (inst.idSrc !== 0 && world.posOf(inst.idSrc, tmp)) {
          copy3(inst.pos, tmp);
          inst.positioned = true;
        }
        break;
      case 'fixed':
        if (inst.idSrc !== 0 && world.posOf(inst.idSrc, tmp)) {
          copy3(inst.pos, inst.posDest);
          inst.positioned = true;
        }
        break;
      case 'none':
        inst.positioned = true;
        break;
    }
    return true;
  }

  private visible(
    inst: SfxInstance,
    frame: SfxFrame,
    opts: Pick<RenderOptions, 'sfxLevel' | 'sfxRenderOff' | 'objectDistant'>,
  ): boolean {
    if (opts.sfxRenderOff && inst.renderFlag !== 1) return false;
    // `m_nSfxLevel <= 0`: the non-skill effects of other players are hidden; `SFXRenderOff`: their skill effects
    if (inst.idSrc !== 0 && frame.world.isPlayer(inst.idSrc)) {
      if (opts.sfxLevel <= 0 && inst.cls !== 'skill') return false;
      if (opts.sfxRenderOff && inst.cls === 'skill') return false;
    }
    const proto = inst.proto;
    if (!proto) return false;
    // the bounding sphere (PL-123), centre mapped to three.js space; the emitter offset of particles is not scaled. A MESH part
    // reaches as far as its model does (`extentOf`, the fallback while the model loads): #018 B9
    let base = proto.radius;
    const model = inst.model;
    if (model) {
      for (const part of meshPartsOf(proto)) {
        const path = model.meshPathOf(part);
        if (path === null) continue;
        const extent = this.deps.meshes?.extentOf?.(path) ?? MESH_EXTENT_FALLBACK_M;
        base = Math.max(base, meshPartRadius(part, extent));
      }
    }
    const s = Math.max(inst.scale[0], inst.scale[1], inst.scale[2], 1);
    const r = base * s + 0.5;
    const cam = frame.cameraPos;
    const dx = inst.pos[0] - cam[0];
    const dy = inst.pos[1] - cam[1];
    const dz = inst.pos[2] - cam[2];
    const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
    // the distance class counts to the surface of the sphere: a big effect near the player is not cut by its pivot (#018)
    if (Math.max(0, dist - r) > cullDistance(opts.objectDistant, inst.distant)) return false;
    return frame.frustum.intersectsSphere(
      SPHERE.set(inst.pos[0], inst.pos[1] + inst.spec.lift, -inst.pos[2], r),
    );
  }

  // ---- housekeeping ------------------------------------------------------------------------------------------------

  /** Prototypes (and their first textures) of a list of effect ids: the common effects after the world entry (Plan/41 16.1). */
  preload(ids: Iterable<number>): void {
    for (const xi of ids) {
      const file = this.deps.catalog.row(xi)?.file ?? null;
      if (file === null) continue;
      void this.deps.prototypes.whenReady(file, Prio.Fx).then((proto) => {
        if (proto) for (const path of this.firstTexturesOf(proto)) this.deps.textures.idOf(path);
      });
    }
  }

  /** Drops every instance (a world change: effects of the old world are gone). */
  clear(): void {
    for (const inst of this.instances) inst.dead = true;
    this.sweep();
    this.marks.clear();
  }

  dispose(): void {
    this.disposed = true;
    this.instances.length = 0;
    this.byHandle.clear();
  }

  /** The world of the last frame (the creation-time lookups of the factory). */
  get lastWorld(): WorldQuery {
    return this.world;
  }

  private get world(): WorldQuery {
    return this.shared.world;
  }

  private set world(w: WorldQuery) {
    this.shared.world = w;
  }

  /** The game's answers for the skill classes (party skill effects, duel state, ...): set by the host after construction. */
  get hooks(): SkillFxHooks {
    return this.shared.hooks;
  }

  set hooks(h: SkillFxHooks) {
    this.shared.hooks = h;
  }
}

// A reusable sphere in three.js space for the frustum test.
const SPHERE = {
  s: new Sphere(new Vector3(), 1),
  set(x: number, y: number, z: number, r: number): Sphere {
    this.s.center.set(x, y, z);
    this.s.radius = r;
    return this.s;
  },
};

function copy3(o: V3, a: ArrayLike<number>): void {
  o[0] = a[0] ?? 0;
  o[1] = a[1] ?? 0;
  o[2] = a[2] ?? 0;
}

export { directionRot3, surfaceRot3 } from './orient';
