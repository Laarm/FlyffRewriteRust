// The placement ghost (`CDeployManager` / `CGuildDeployManager` model, `Housing.cpp:685-1390`, `Obj.cpp`, `Neuz.cpp:1426-1447`; Plan/41 WP-REND-27 ghost part, Plan/45 §28.10, §28.11; WP-CFEAT-46): ONE translucent furniture model that
// follows the pointer while a furniture is placed, and the geometry the placement collision reads (`features/events/deploy/collide.ts`).
//
//   show( ctrlId )      loads the ctrl model (`models.types["2"]` row of the ctrl id) into ONE `ModelInstance` (not pickable, no shadow; hidden until the first pose); a second call replaces it
//   pose( pos, a, c )   `Scale( 1 ) * RotYawPitchRoll( -angle ) * Translate( pos )` (the placement matrix of a ctrl, LH), model blend 80; while colliding blend 110 and the texture `red.tga` on block 0 of EVERY object
//                       (FIX: the original restored every object to the texture of the FIRST object when the ghost left the collision)
//   hide()              the instance goes
//   scene( wall, id )   the collision input: the ghost's box and triangles (LH, object-local, the whole LOD-0 hierarchy merged), the room wall object (`obj_miniwall01.o3d`: the first placement of the world with its box and
//                       triangles), the boxes of the other ctrls except `id`; `null` while the ghost model is loading
//   wallPresent( file ) the world has a placement of `file` (a guild placement refuses to start without the wall)
//
// The triangles are the LOD-0 meshes of the decoded model (the collision object of the original is not kept by the decoder); a light object is skipped. Node animation is ignored (the bind pose).
// This module is importable from `src/render/**` only (R15).

import { Matrix4, Vector3, type Object3D } from 'three';
import type { DeployBody, DeployMesh, DeployRenderApi, DeployScene } from '../../game/ports';
import { type TextureNameRules } from '../gl/textureNames';
import { mirrorAabb } from '../space';
import type { Vec3LH } from '../types';
import { Prio } from '../../workers/priority';
import type { ModelGeometry } from './ModelCache';
import type { AlternateTexture, ModelInstanceOptions } from './ModelInstance';
import { worldMatrixThree } from './placement';

/** The model blend of the ghost (`SetBlendFactor( 80 )`), and the one while it collides (`110`). */
export const GHOST_BLEND = 80;
export const GHOST_BLEND_RED = 110;
/** The texture of a colliding ghost (`Model/Texture/red.tga`, key `0xffff00ff`; the file has no magenta pixel, so the PNG without a key is exact). */
export const GHOST_RED_TEXTURE = 'red.tga';

/** What the ghost needs of a model instance (`ModelInstance` implements it). */
export interface GhostInstanceLike {
  readonly root: Object3D;
  setTransform(m: Matrix4): void;
  setModelBlend(blend: number): void;
  addAlternate(objectIndex: number, blockIndex: number, rules: TextureNameRules): AlternateTexture;
}

export interface DeployGhostDeps {
  /** The model row of a ctrl id (`models.types["2"]`), `undefined` when unknown. */
  readonly modelOf: (ctrlId: number) => { readonly file: string | null } | undefined;
  readonly models: { model(path: string, prio?: Prio): Promise<ModelGeometry | null> };
  readonly host: {
    createInstance(
      model: ModelGeometry,
      options: ModelInstanceOptions,
      parent?: Object3D,
    ): GhostInstanceLike;
    removeInstance(inst: GhostInstanceLike): void;
  };
  /** The placements of a static model file (`StaticObjects.instancesOf`). */
  readonly staticInstances: (file: string) => readonly {
    readonly matrix: readonly number[];
    readonly geometry: ModelGeometry;
  }[];
  /** The boxes of the ctrls of the world (`Ctrls.worldBoxes`). */
  readonly ctrlBoxes: () => readonly {
    readonly id: number;
    readonly matrix: readonly number[];
    readonly bbox: { readonly min: Vec3LH; readonly max: Vec3LH };
  }[];
  readonly log?: (message: string) => void;
}

interface GhostState {
  readonly ctrlId: number;
  model: ModelGeometry | null;
  inst: GhostInstanceLike | null;
  alternates: AlternateTexture[];
  /** The last requested pose (applied when the model arrives). */
  pos: Vec3LH | null;
  angle: number;
  collide: boolean;
}

const V = new Vector3();
const M = new Matrix4();

/** The chain matrices (`parent * local`, three.js space) of the LOD-0 objects: the model-space placement of each object. */
function chains(model: ModelGeometry): Matrix4[] {
  const out: Matrix4[] = [];
  for (const obj of model.lods[0] ?? []) {
    const local = obj.kind === 'rigid' ? obj.localTM : new Matrix4();
    const parent = obj.parent.kind === 'object' ? out[obj.parent.index] : undefined;
    out.push(parent ? parent.clone().multiply(local) : local.clone());
  }
  return out;
}

const meshCache = new WeakMap<ModelGeometry, DeployMesh>();

/**
 * The LOD-0 triangles of a model in LH object-local (model) space: every non-light object through its chain matrix, the z axis mirrored back (the geometry is in three.js space), one merged list.
 * (The winding is reversed by the mirror; the triangle test does not care.)
 */
export function modelMesh(model: ModelGeometry): DeployMesh {
  const have = meshCache.get(model);
  if (have) return have;
  const chain = chains(model);
  const positions: number[] = [];
  const index: number[] = [];
  (model.lods[0] ?? []).forEach((obj, i) => {
    if (obj.light) return;
    const base = positions.length / 3;
    const m = chain[i] ?? M.identity();
    const p = obj.pick.positions;
    for (let k = 0; k + 2 < p.length; k += 3) {
      V.set(p[k] ?? 0, p[k + 1] ?? 0, p[k + 2] ?? 0).applyMatrix4(m);
      positions.push(V.x, V.y, -V.z);
    }
    const idx = obj.pick.index;
    for (const v of idx) index.push(base + v);
  });
  const mesh: DeployMesh = {
    positions: Float32Array.from(positions),
    index: Uint32Array.from(index),
  };
  meshCache.set(model, mesh);
  return mesh;
}

/** The LH object-local box of a model (`mirrorAabb` of the decoded header box). */
export function modelBox(model: ModelGeometry): { min: Vec3LH; max: Vec3LH } {
  const b = model.bbox;
  const [min, max] = mirrorAabb([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]);
  return { min, max };
}

export class DeployGhost implements DeployRenderApi {
  private state: GhostState | null = null;
  private disposed = false;
  /** The poses applied (tests, probes). */
  poses = 0;

  constructor(private readonly deps: DeployGhostDeps) {}

  /** The state of the ghost (tests, probes). */
  probe(): {
    ctrlId: number;
    loaded: boolean;
    visible: boolean;
    collide: boolean;
    pos: Vec3LH | null;
    angle: number;
  } | null {
    const s = this.state;
    return s
      ? {
          ctrlId: s.ctrlId,
          loaded: s.inst !== null,
          visible: s.inst?.root.visible ?? false,
          collide: s.collide,
          pos: s.pos,
          angle: s.angle,
        }
      : null;
  }

  show(ctrlId: number): void {
    if (this.disposed) return;
    this.hide();
    const s: GhostState = {
      ctrlId,
      model: null,
      inst: null,
      alternates: [],
      pos: null,
      angle: 0,
      collide: false,
    };
    this.state = s;
    const row = this.deps.modelOf(ctrlId);
    if (!row?.file) {
      this.deps.log?.(`deploy ghost: ctrl ${String(ctrlId)} has no model`);
      return;
    }
    const file = row.file;
    this.deps.models.model(file, Prio.Visible).then(
      (model) => {
        if (this.disposed || this.state !== s) return;
        if (!model) {
          this.deps.log?.(`deploy ghost: ${file} does not exist`);
          return;
        }
        this.ready(s, model);
      },
      (e: unknown) => {
        this.deps.log?.(`deploy ghost: ${e instanceof Error ? e.message : String(e)}`);
      },
    );
  }

  private ready(s: GhostState, model: ModelGeometry): void {
    s.model = model;
    const inst = this.deps.host.createInstance(model, { modelBlend: GHOST_BLEND });
    inst.root.visible = false;
    s.inst = inst;
    // the red texture on block 0 of every object (FIX: the original restored the texture of the first object for all)
    (model.lods[0] ?? []).forEach((obj, i) => {
      if (obj.blocks.length === 0) return;
      s.alternates.push(inst.addAlternate(i, 0, { literal: GHOST_RED_TEXTURE }));
    });
    this.apply(s);
  }

  pose(pos: Vec3LH, angleDeg: number, collide: boolean): void {
    const s = this.state;
    if (!s || this.disposed) return;
    s.pos = [pos[0], pos[1], pos[2]];
    s.angle = angleDeg;
    s.collide = collide;
    this.apply(s);
  }

  private apply(s: GhostState): void {
    const inst = s.inst;
    if (!inst || !s.pos) return;
    worldMatrixThree({ pos: s.pos, angle: s.angle, angX: 0, angZ: 0, scale: [1, 1, 1] }, M);
    inst.setTransform(M);
    inst.setModelBlend(s.collide ? GHOST_BLEND_RED : GHOST_BLEND);
    for (const a of s.alternates) a.set(s.collide);
    inst.root.visible = true;
    this.poses++;
  }

  hide(): void {
    const s = this.state;
    this.state = null;
    if (s?.inst) this.deps.host.removeInstance(s.inst);
  }

  wallPresent(file: string): boolean {
    return this.deps.staticInstances(file).length > 0;
  }

  scene(wallFile: string, exceptObjId: number): DeployScene | null {
    const s = this.state;
    if (!s?.model) return null;
    const ghost = { bbox: modelBox(s.model), mesh: modelMesh(s.model) };
    const first = this.deps.staticInstances(wallFile)[0];
    const wall: DeployBody | null = first
      ? { matrix: first.matrix, bbox: modelBox(first.geometry), mesh: modelMesh(first.geometry) }
      : null;
    const ctrls = this.deps.ctrlBoxes().filter((c) => c.id !== exceptObjId);
    return { ghost, wall, ctrls };
  }

  dispose(): void {
    this.hide();
    this.disposed = true;
  }
}
