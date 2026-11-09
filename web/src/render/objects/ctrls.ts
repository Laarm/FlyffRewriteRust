// Controls (OT_CTRL: chests, doors, traps, triggers, guild-house furniture; Plan/41 section 11.8, WP-REND-23; `CCommonCtrl::Render`,
// CommonCtrl.cpp, `CDPClient::OnCommonPlace`): the model of a ctrl, its node animation and its fade - cosmetic only.
//
//   model      `models.types["2"]` row of the ctrl id -> `.o3d`; the LOD-0 objects only (node clips hold LOD 0 only, 31 section 13.7)
//   placement  `Scale * RotYawPitchRoll(-angle) * Translate(pos)` of the `SpawnCtrl` (the matrix the game gave the sim in `CTRL_ADD`);
//              an entry with a pose uses the world matrix of its decor record
//   animation  node matrices come from the sim: a ctrl whose model animates has an implicit entry (`CTRL_ADD` of the spawn handler);
//              its `CTRL_STATE` frame is the game's. The renderer says whether the ctrl is drawn (`SET_VISIBLE`) and reads the node
//              matrices of the `decor` area; until the entry has a pose (clip not loaded yet) the model stands in frame 0
//   clips      the node clip (`FMO1`) of the decoded `.o3d` is the built-in clip of the model in the motion table: it is handed to the
//              sim once per model (`add_clip`), the rest of the motion table is the server's `/gen/motion-table`
//   fade       `CommonPlace{ALPHA}` starts `m_bAlpha`: the blend factor drops 5 per 1/60 s tick from 255 (CK2_FADE ctrls; the server
//              deletes the ctrl when it reaches zero)
//   effect     `CommonPlace{ACTION}` of a ctrl that does not play yet creates `CtrlProp.dwSfxCtrl` at the ctrl (`SetActionPlay`)
//   picking    ctrls are picked by their box and, precisely, by the triangles of the current node pose (Plan/41 section 20.3)
//
// This module is importable from `src/render/**` only (R15).

import { Matrix4, Sphere, Vector3, type Frustum, type Object3D } from 'three';
import { OT_CTRL } from '../../gen/defs/define';
import type { SfxOptions } from '../fx';
import { XE } from '../gl/materials/key';
import { rayVsMesh, type PickMesh } from '../pick/meshRay';
import type { PickSource } from '../pick/PickSupport';
import type { EntityViewSource, ObjId, PickProxy, Ray3 } from '../types';
import { Prio } from '../../workers/priority';
import { DecorView, nodeLocals, type DecorSink, type NodeParent } from './decor';
import { cullDistance } from './distanceTable';
import type { ModelGeometry } from './ModelCache';
import type { ModelInstanceOptions } from './ModelInstance';
import { worldMatrixLH, worldMatrixThree, worldSphere } from './placement';
import { mirrorAabb } from '../space';

/** The entity kind of ctrls (`EntityKind.Ctrl`). */
const CTRL_KIND = 4;
/** `COMMONPLACE_*` (CommonCtrl.h). */
export const COMMONPLACE = { ACTION: 0, ALPHA: 1, QUAKE: 2 } as const;
/** `m_nBlendFactorLocal -= 5` per client tick of a fading ctrl. */
export const FADE_STEP = 5;
/** `NULL_ID` of `CtrlProp.dwSfxCtrl`: no effect. */
const NO_SFX = 0xffff_ffff;

export type V3 = readonly [number, number, number];

/** What a ctrl needs of the renderer (`WorldRenderer` implements it with `ModelInstance`). */
export interface CtrlInstanceLike {
  readonly root: Object3D;
  setTransform(m: Matrix4): void;
  /** The model blend factor 0..255. */
  setModelBlend(blend: number): void;
  /** The matrix of node `objectIndex` relative to its parent node. */
  setNodeMatrix(objectIndex: number, m: Matrix4): void;
  /** The node group of object `objectIndex` (its `matrixWorld` is the pose for the precise pick). */
  nodeOf(objectIndex: number): Object3D | null;
}

export interface CtrlsDeps {
  /** The model row of a ctrl id (`models.types["2"]`), `undefined` when unknown. */
  readonly modelOf: (
    ctrlId: number,
  ) => { readonly file: string | null; readonly distant?: number } | undefined;
  /** `CtrlProp.dwSfxCtrl` of a ctrl id (`NULL_ID` / `undefined`: none). */
  readonly sfxOf?: (ctrlId: number) => number | undefined;
  readonly models: { model(path: string, prio?: Prio): Promise<ModelGeometry | null> };
  readonly host: {
    createInstance(
      model: ModelGeometry,
      options: ModelInstanceOptions,
      parent?: Object3D,
    ): CtrlInstanceLike;
    removeInstance(inst: CtrlInstanceLike): void;
  };
  /** The sim side (`SET_VISIBLE`, the node clips); without it ctrls stand in their bind pose. */
  readonly decor?: DecorSink;
  /** Starts an effect (`CtrlProp.dwSfxCtrl`). */
  readonly sfx: (xi: number, o: SfxOptions) => void;
  /** `RenderOptions.objectDistant`. */
  readonly objectDistant: () => 0 | 1 | 2;
  readonly log?: (message: string) => void;
}

/** What the renderer reads of a ctrl record of the entity table. */
interface CtrlRecord {
  readonly objId: number;
  readonly kind?: number;
  readonly propId?: number;
  readonly removed?: boolean;
  readonly spawn?: {
    readonly obj?: {
      readonly index?: number;
      readonly scaleX100?: number;
      readonly angleX10?: number;
      readonly pos?: { readonly x: number; readonly y: number; readonly z: number };
    };
  };
}

interface Ctrl {
  readonly id: number;
  readonly index: number;
  readonly pos: V3;
  readonly scale: number;
  /** Degrees, LH (the quantised `angle_x10 / 10` of the spawn). */
  readonly angle: number;
  inst: CtrlInstanceLike | null;
  model: ModelGeometry | null;
  state: 'loading' | 'ready' | 'failed';
  /** Node parents of the LOD-0 objects (for the local matrices). */
  nodes: NodeParent[];
  distant: number;
  drawn: boolean;
  /** What the sim was told last (`null` = nothing yet). */
  visibleSent: boolean | null;
  /** `CommonPlace{ALPHA}` arrived: the ctrl fades out. */
  alpha: boolean;
  /** `m_nBlendFactorLocal`. */
  blend: number;
  /** The ctrl plays (`m_bAniPlay`): its effect was created. */
  played: boolean;
  /** `frame_cur` of the sim entry in the last frame, `-1` without an entry. */
  frame: number;
  /** The last frame had an evaluated pose. */
  posed: boolean;
  /** The node clip was handed to the sim. */
  clipSent: boolean;
  readonly matrix: Matrix4;
}

/** One row of {@link Ctrls.probe}. */
export interface CtrlProbeRow {
  readonly id: number;
  readonly index: number;
  readonly pos: readonly [number, number, number];
  readonly state: 'loading' | 'ready' | 'failed';
  readonly drawn: boolean;
  readonly hasInstance: boolean;
  readonly hasModel: boolean;
  readonly blend: number;
  readonly frame: number;
  /** The model box `[minx, miny, minz, maxx, maxy, maxz]`, `null` before the model is resident. */
  readonly box: readonly number[] | null;
}

export interface CtrlStats {
  ctrls: number;
  /** Ctrls drawn in the last frame. */
  drawn: number;
  /** Ctrls whose node matrices came from the sim in the last frame. */
  posed: number;
  loading: number;
  failed: number;
  /** `SET_VISIBLE` records sent. */
  visibleSent: number;
  /** Node clips handed to the sim. */
  clips: number;
  fades: number;
  effects: number;
}

const SPHERE = new Sphere();
/** Scratch of the culling sphere (three.js space) and of the placement matrix of a ctrl that was not posed yet. */
const WORLD = { center: new Vector3(), radius: 0 };
const PLACED = new Matrix4();
const LOCAL = new Matrix4();

export class Ctrls {
  private readonly ctrls = new Map<number, Ctrl>();
  private readonly decorView = new DecorView();
  private readonly scratch: Matrix4[] = [];
  private readonly clipsSent = new Set<number>();
  private version = -1;
  private disposed = false;
  readonly stats: CtrlStats = {
    ctrls: 0,
    drawn: 0,
    posed: 0,
    loading: 0,
    failed: 0,
    visibleSent: 0,
    clips: 0,
    fades: 0,
    effects: 0,
  };

  constructor(private readonly deps: CtrlsDeps) {}

  has(id: ObjId): boolean {
    return this.ctrls.has(id);
  }

  /** The visible state of a ctrl (tests, probes). */
  stateOf(id: ObjId): {
    state: Ctrl['state'];
    drawn: boolean;
    frame: number;
    posed: boolean;
    blend: number;
    alpha: boolean;
    played: boolean;
    visibleSent: boolean | null;
  } | null {
    const c = this.ctrls.get(id);
    return c
      ? {
          state: c.state,
          drawn: c.drawn,
          frame: c.frame,
          posed: c.posed,
          blend: c.blend,
          alpha: c.alpha,
          played: c.played,
          visibleSent: c.visibleSent,
        }
      : null;
  }

  /** The probe rows (`__flyffDebug.render.ctrls`): one row per ctrl with its load state, whether it is drawn and its pick box. */
  probe(): { stats: CtrlStats; rows: CtrlProbeRow[] } {
    const rows: CtrlProbeRow[] = [];
    for (const c of this.ctrls.values()) {
      const b = c.model?.bbox;
      rows.push({
        id: c.id,
        index: c.index,
        pos: [c.pos[0], c.pos[1], c.pos[2]],
        state: c.state,
        drawn: c.drawn,
        hasInstance: c.inst !== null,
        hasModel: c.model !== null,
        blend: c.blend,
        frame: c.frame,
        box: b ? [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z] : null,
      });
    }
    return { stats: { ...this.stats }, rows };
  }

  // ---- CommonPlace -----------------------------------------------------------------------------------------------

  /** `CDPClient::OnCommonPlace`: `kind` is a `COMMONPLACE_*`. */
  commonPlace(id: ObjId, kind: number): void {
    const c = this.ctrls.get(id);
    if (!c) return;
    if (kind === COMMONPLACE.ALPHA) {
      if (!c.alpha) this.stats.fades++;
      c.alpha = true;
    } else if (kind === COMMONPLACE.ACTION) {
      // `SetActionPlay` on the client: `if (!m_bAniPlay)` creates the ctrl effect
      if (!c.played) {
        c.played = true;
        const xi = this.deps.sfxOf?.(c.index);
        if (xi !== undefined && xi !== NO_SFX && xi !== 0) {
          this.stats.effects++;
          this.deps.sfx(xi, { pos: [c.pos[0], c.pos[1], c.pos[2]] });
        }
      }
    }
  }

  // ---- the frame -------------------------------------------------------------------------------------------------

  /** Syncs the ctrls with the entity table, poses the visuals from the decor area of `view` and places them. */
  update(
    entities: EntityViewSource | null,
    view: unknown,
    cameraPos: V3,
    frustum: Frustum | null,
  ): void {
    if (this.disposed) return;
    this.reconcile(entities);
    const haveView = this.decorView.bind(view);
    const opt = this.deps.objectDistant();
    let drawn = 0;
    let posed = 0;
    let loading = 0;
    let failed = 0;
    for (const c of this.ctrls.values()) {
      if (c.state === 'loading') loading++;
      if (c.state === 'failed') failed++;
      const inst = c.inst;
      if (!inst) {
        c.drawn = false;
        continue;
      }
      const rec = haveView ? this.decorView.find(c.id, true) : -1;
      c.frame = rec >= 0 ? this.decorView.frameCur(rec) : -1;
      let dist = Math.hypot(
        c.pos[0] - cameraPos[0],
        c.pos[1] - cameraPos[1],
        c.pos[2] - cameraPos[2],
      );
      if (c.model) {
        // the sphere of the model box under the FULL matrix (the pose of an earlier frame, else the placement): a door hinged at
        // one side or a wide chest has its centre off the pivot in x/z and under rotation (B7); an inverted header box used to
        // give radius 0.3. The distance class measures to the sphere surface, like the statics (large ctrls never vanish).
        worldSphere(c.model.bbox, c.posed ? c.matrix : this.placementOf(c, PLACED), WORLD);
        WORLD.radius += 0.3;
        // `cameraPos` is LH, the sphere is in three.js space (z mirrored)
        dist = Math.max(
          0,
          Math.hypot(
            WORLD.center.x - cameraPos[0],
            WORLD.center.y - cameraPos[1],
            WORLD.center.z + cameraPos[2],
          ) - WORLD.radius,
        );
      }
      let visible = c.blend > 0 && dist <= cullDistance(opt, c.distant);
      if (visible && frustum && c.model) {
        SPHERE.set(WORLD.center, WORLD.radius);
        visible = frustum.intersectsSphere(SPHERE);
      }
      inst.root.visible = visible;
      c.drawn = visible;
      // the sim evaluates the pose only for the entries it is told are drawn
      if (rec >= 0 && this.deps.decor && c.visibleSent !== visible) {
        this.deps.decor.setCtrlVisible(c.id, visible);
        c.visibleSent = visible;
        this.stats.visibleSent++;
      }
      if (!visible) continue;
      drawn++;
      c.posed = rec >= 0 && this.decorView.hasPose(rec);
      if (c.posed) {
        posed++;
        this.decorView.world(rec, c.matrix);
        this.poseNodes(c, inst, rec);
      } else {
        this.placeAt(c);
      }
      inst.setTransform(c.matrix);
    }
    this.stats.ctrls = this.ctrls.size;
    this.stats.drawn = drawn;
    this.stats.posed = posed;
    this.stats.loading = loading;
    this.stats.failed = failed;
  }

  /** The placement matrix of the spawn (three.js space) written into `out`. */
  private placementOf(c: Ctrl, out: Matrix4): Matrix4 {
    return worldMatrixThree(
      {
        pos: [c.pos[0], c.pos[1], c.pos[2]],
        angle: c.angle,
        angX: 0,
        angZ: 0,
        scale: [c.scale, c.scale, c.scale],
      },
      out,
    );
  }

  private placeAt(c: Ctrl): void {
    this.placementOf(c, c.matrix);
  }

  private poseNodes(c: Ctrl, inst: CtrlInstanceLike, rec: number): void {
    const view = this.decorView;
    nodeLocals(
      c.nodes,
      (k, out) => view.matrix(rec, k, out),
      (i, local) => {
        LOCAL.copy(local);
        inst.setNodeMatrix(i, LOCAL);
      },
      this.scratch,
    );
  }

  private reconcile(entities: EntityViewSource | null): void {
    if (!entities) return;
    const v = entities.version ?? 0;
    if (v === this.version && this.ctrls.size > 0) return;
    this.version = v;
    const forEach = (
      entities as {
        forEach?: (kind: number, fn: (r: CtrlRecord) => void) => void;
      }
    ).forEach;
    if (!forEach) return;
    const live = new Set<number>();
    forEach.call(entities, CTRL_KIND, (r) => {
      if (r.removed === true || (r.kind !== undefined && r.kind !== CTRL_KIND)) return;
      live.add(r.objId);
      if (!this.ctrls.has(r.objId)) this.add(r);
    });
    for (const [id, c] of this.ctrls) {
      if (live.has(id)) continue;
      this.remove(c);
      this.ctrls.delete(id);
    }
  }

  private add(r: CtrlRecord): void {
    const obj = r.spawn?.obj;
    const p = obj?.pos;
    if (!p) return;
    const index = obj.index ?? r.propId ?? 0;
    const scale100 = obj.scaleX100 ?? 100;
    const c: Ctrl = {
      id: r.objId,
      index,
      pos: [p.x, p.y, p.z],
      scale: scale100 > 0 ? scale100 / 100 : 1,
      angle: (obj.angleX10 ?? 0) / 10,
      inst: null,
      model: null,
      state: 'loading',
      nodes: [],
      distant: 0,
      drawn: false,
      visibleSent: null,
      alpha: false,
      blend: 255,
      played: false,
      frame: -1,
      posed: false,
      clipSent: false,
      matrix: new Matrix4(),
    };
    this.ctrls.set(r.objId, c);
    this.load(c);
  }

  // ---- the model -------------------------------------------------------------------------------------------------

  private load(c: Ctrl): void {
    const row = this.deps.modelOf(c.index);
    c.distant = row?.distant ?? 0;
    if (!row?.file) {
      this.fail(c, 'no model');
      return;
    }
    this.deps.models.model(row.file, Prio.Visible).then(
      (geometry) => {
        if (this.disposed || this.ctrls.get(c.id) !== c) return;
        if (!geometry) this.fail(c, `${row.file ?? ''} does not exist`);
        else this.ready(c, geometry);
      },
      (e: unknown) => {
        this.fail(c, e instanceof Error ? e.message : String(e));
      },
    );
  }

  private fail(c: Ctrl, why: string): void {
    c.state = 'failed';
    this.deps.log?.(`ctrl ${String(c.index)}: ${why}`);
  }

  private ready(c: Ctrl, model: ModelGeometry): void {
    c.model = model;
    c.state = 'ready';
    c.nodes = (model.lods[0] ?? []).map((o) => ({
      parent: o.parent.kind === 'object' ? o.parent.index : -1,
    }));
    c.inst = this.deps.host.createInstance(model, {});
    c.inst.root.visible = false;
    if (c.blend < 255) c.inst.setModelBlend(c.blend);
    this.placeAt(c);
    // the node clip of the decoded model is the built-in clip the sim animates the ctrl with (31 section 13.6)
    this.sendClip(c, model);
  }

  private sendClip(c: Ctrl, model: ModelGeometry): void {
    const decor = this.deps.decor;
    const clip = model.nodeClip;
    if (!decor || !clip || c.clipSent) return;
    const id = decor.builtinClip(OT_CTRL, c.index);
    if (id === null) return;
    c.clipSent = true;
    if (this.clipsSent.has(id)) return;
    this.clipsSent.add(id);
    this.stats.clips++;
    decor.loadClip(id, clip);
  }

  private remove(c: Ctrl): void {
    if (c.inst) this.deps.host.removeInstance(c.inst);
    c.inst = null;
  }

  // ---- housing / guild house (ui-house-quiz)
  /**
   * The boxes of the ctrls whose model is loaded, for the placement collision of the furniture ghost (`CWorld::ProcessObjCollision`: the other furniture of the room): the LH world matrix in D3D row-major order
   * (`Scale * RotYawPitchRoll(-angle) * Translate(pos)` of the spawn) and the LH object-local box of the model.
   */
  worldBoxes(): { id: number; matrix: number[]; bbox: { min: V3; max: V3 } }[] {
    const out: { id: number; matrix: number[]; bbox: { min: V3; max: V3 } }[] = [];
    for (const c of this.ctrls.values()) {
      const b = c.model?.bbox;
      if (!b || c.state !== 'ready') continue;
      const [min, max] = mirrorAabb([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]);
      out.push({
        id: c.id,
        matrix: worldMatrixLH({
          pos: [c.pos[0], c.pos[1], c.pos[2]],
          angle: c.angle,
          angX: 0,
          angZ: 0,
          scale: [c.scale, c.scale, c.scale],
        }),
        bbox: { min, max },
      });
    }
    return out;
  }

  // ---- the 60 Hz step ---------------------------------------------------------------------------------------------

  /** `CCommonCtrl::Process` on the client: a fading ctrl loses 5 of its blend factor per tick. */
  tick(steps: number): void {
    if (this.disposed) return;
    for (let k = 0; k < steps; k++) {
      for (const c of this.ctrls.values()) {
        if (!c.alpha || c.blend <= 0) continue;
        c.blend = Math.max(0, c.blend - FADE_STEP);
        c.inst?.setModelBlend(c.blend);
      }
    }
  }

  // ---- picking ---------------------------------------------------------------------------------------------------

  /** The pick source of the ctrls drawn in the last frame (`PickSupport.register`): the box and the precise triangle test. */
  pickSource(): PickSource {
    const aabbOf = (c: Ctrl): readonly [V3, V3] => {
      const b = c.model?.bbox;
      return b
        ? [
            [b.min.x, b.min.y, b.min.z],
            [b.max.x, b.max.y, b.max.z],
          ]
        : [
            [-0.5, 0, -0.5],
            [0.5, 1, 0.5],
          ];
    };
    const matrixOf = (c: Ctrl, out: Matrix4): Matrix4 => this.placementOf(c, out);
    return {
      collect: (out: PickProxy[]) => {
        for (const c of this.ctrls.values()) {
          if (!c.drawn) continue;
          const aabb = aabbOf(c);
          out.push({
            id: c.id,
            kind: 'ctrl',
            worldMatrix: matrixOf(c, new Matrix4()),
            localAabb: [aabb[0], aabb[1]],
            dead: false,
            isPlayer: false,
            rank: 0,
            ai: 0,
            ownPet: false,
            transparent: false,
            gcwarNotClick: false,
          });
        }
      },
      boundsOf: (id: number) => {
        const c = this.ctrls.get(id);
        if (!c?.drawn) return undefined;
        const aabb = aabbOf(c);
        return {
          matrix: matrixOf(c, new Matrix4()),
          aabb: [[...aabb[0]], [...aabb[1]]] as readonly [readonly number[], readonly number[]],
        };
      },
      raycast: (id: number, ray: Ray3) => {
        const c = this.ctrls.get(id);
        if (!c?.drawn || !c.inst || !c.model) return undefined;
        return this.preciseHit(c, ray);
      },
    };
  }

  /** Every LOD-0 object through the world matrix of its node group (the current pose). */
  private preciseHit(c: Ctrl, ray: Ray3): number | null {
    const inst = c.inst;
    const model = c.model;
    if (!inst || !model) return null;
    let best: number | null = null;
    (model.lods[0] ?? []).forEach((obj, i) => {
      if (obj.light || obj.kind === 'skin') return;
      const world = inst.nodeOf(i)?.matrixWorld;
      if (!world) return;
      const mesh: PickMesh = {
        positions: obj.pick.positions,
        index: obj.pick.index,
        joints: null,
        blocks: obj.blocks.map((b) => ({
          start: b.submesh.start,
          count: b.submesh.count,
          twoSided: (b.submesh.effect & XE.TWO_SIDE) !== 0,
        })),
      };
      const t = rayVsMesh(ray, mesh, () => world);
      if (t !== null && (best === null || t < best)) best = t;
    });
    return best;
  }

  // ---- lifetime --------------------------------------------------------------------------------------------------

  /** A world change: the ctrls of the old world are gone. */
  clear(): void {
    for (const c of this.ctrls.values()) this.remove(c);
    this.ctrls.clear();
    // (the clips stay in the sim across worlds: `clipsSent` is kept)
    this.version = -1;
    this.stats.ctrls = 0;
    this.stats.drawn = 0;
  }

  dispose(): void {
    this.clear();
    this.disposed = true;
  }
}
