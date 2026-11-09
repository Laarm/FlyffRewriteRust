// MESH parts of the effects (Plan/41 section 16.4, 16.5, WP-REND-19; `CSfxPartMesh::Render`, sfxbase.cpp:1072-1178).
//
// A MESH part draws a model (`model/<name>.o3d`: arrows, doors, the yo-yo shapes of skills) at the world matrix the part code finished
// (`Scale(S3 * k.scale) * YPR(k.rot + (0, a, 0)) * Translate(...)`, `emitMesh`). The geometry is the renderer's ordinary model cache entry
// and the part is a `ModelInstance` in the effect scene, so the textures, the LOD handling and the material system are the model
// pipeline's own:
//
//   BLEND    the normal mesh pipeline (lit, texture x diffuse, alpha test of the block); the key alpha is ignored
//   GLOW     `m_nNoEffect = 1`: colour = texture, alpha = texture alpha x key alpha (the low byte), SRCALPHA / ONE, depth test and
//            depth WRITE on (the quads and rings of the other parts do not write depth), no lighting, no fog
//
// Every (instance, part) owns its `ModelInstance` and its animation clock: the frame of a node animation is a function of the steps
// of THAT effect instance (`meshFrame`, PL-134 - the original advanced one shared mesh once per instance and tick).
//
// Draw order: the GLOW meshes are transparent objects and sort with the quads by `renderOrder`; a BLEND mesh is opaque for three.js
// and is drawn before them (it writes depth, so the effect parts drawn later are depth tested against it as in the original).

import {
  AddEquation,
  BackSide,
  type Box3,
  CustomBlending,
  FrontSide,
  GLSL3,
  Matrix4,
  OneFactor,
  RawShaderMaterial,
  SrcAlphaFactor,
  type Mesh,
  type Object3D,
  type Texture,
  DataTexture,
  RGBAFormat,
  UnsignedByteType,
  Vector3,
} from 'three';
import { Prio } from '../../workers/priority';
import type { MaterialKey } from '../gl/materials/key';
import { det3x3, type ModelMaterial } from '../gl/materials/MaterialFactory';
import type { ModelInstanceOptions } from '../objects/ModelInstance';
import type { ModelGeometry } from '../objects/ModelCache';
import type { MeshItem } from './DrawList';
import { meshFrame } from './meshAnim';
import type { SfxModel } from './SfxModel';
import type { MeshPartHandler } from './SfxRenderer';

/** Frames a slot may stay unplaced (culled, finished) before its instance is released (3 s at 60 fps). */
export const STALE_FRAMES = 180;

/** What a part needs of a model instance (`ModelInstance` satisfies it; the tests fake it). */
export interface MeshInstance {
  readonly root: Object3D;
  readonly model: Pick<ModelGeometry, 'maxFrame' | 'perSlerp'>;
  /** Settles when the textures requested so far are resident or failed (`ModelInstance.ready`). */
  readonly ready?: Promise<void>;
  readonly blocks: readonly { readonly mesh: Mesh }[];
  setTransform(m: Matrix4): void;
  setNodeFrame(frame: number): void;
}

/** What the part handler needs of the renderer (`WorldRenderer.createInstance` / `removeInstance`). */
export interface MeshHost {
  createInstance(
    model: ModelGeometry,
    options: ModelInstanceOptions,
    parent: Object3D,
  ): MeshInstance;
  removeInstance(inst: MeshInstance): void;
}

export interface SfxMeshesDeps {
  /** The models of the client (`ModelProvider.model`): `null` when the file does not exist (the part is invisible, KQ-30). */
  readonly models: { model(path: string, prio?: Prio): Promise<ModelGeometry | null> };
  readonly host: MeshHost;
  /** Where the instances go: the scene of the effect pass. */
  readonly parent: Object3D;
  readonly log?: (message: string) => void;
}

export interface SfxMeshStats {
  /** Slots (one per instance and MESH part) alive. */
  slots: number;
  /** Slots whose model is being loaded. */
  loading: number;
  /** Slots whose model does not exist or cannot be decoded. */
  failed: number;
  /** Slots placed in the last frame. */
  placed: number;
}

interface Slot {
  readonly path: string;
  readonly glow: boolean;
  inst: MeshInstance | null;
  state: 'loading' | 'ready' | 'failed';
  /** The instance exists but its textures are still being requested. */
  texturesPending: boolean;
  placed: boolean;
  idle: number;
  dead: boolean;
}

const GLOW_VERTEX = /* glsl */ `
precision highp float;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
in vec3 position;
in vec2 uv;
out vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;

const GLOW_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
uniform float uAlpha;
in vec2 vUv;
layout(location = 0) out highp vec4 fragColor;
void main() {
  vec4 t = texture(uMap, vUv);
  fragColor = vec4(t.rgb, t.a * uAlpha);
}
`;

/** The material of a GLOW mesh block (sfxbase.cpp:2429-2505 with `m_nNoEffect = 1`): additive, depth test and write, no lighting. */
export function createGlowMaterial(
  key: MaterialKey,
  map: Texture,
  alpha: number,
  name = 'sfx-mesh-glow',
): ModelMaterial {
  const m = new RawShaderMaterial({
    name,
    glslVersion: GLSL3,
    vertexShader: GLOW_VERTEX,
    fragmentShader: GLOW_FRAGMENT,
    uniforms: { uMap: { value: map }, uAlpha: { value: alpha } },
    side: FrontSide,
    transparent: true,
    blending: CustomBlending,
    blendEquation: AddEquation,
    blendSrc: SrcAlphaFactor,
    blendDst: OneFactor,
    depthTest: true,
    depthWrite: true,
  }) as ModelMaterial;
  m.userData = { key, keyString: name };
  // KEEP-QUIRK of the model pipeline: the culling of a node that mirrors (negative determinant) is reversed
  m.onBeforeRender = (_r, _s, _c, _g, object: Object3D) => {
    m.side = det3x3(object.matrixWorld) < 0 ? BackSide : FrontSide;
  };
  return m;
}

/** How far a model box reaches from the pivot: `|centre| + |size| / 2` (0 for an empty box). */
function extentOfBox(box: Box3): number {
  if (box.isEmpty()) return 0;
  return box.getCenter(_centre).length() + box.getSize(_size).length() / 2;
}
const _centre = new Vector3();
const _size = new Vector3();

/** The three.js world matrix of a finished LH part matrix (`S * M * S`, 12 numbers -> column-major elements). */
export function partMatrix(world: ArrayLike<number>, out: Matrix4): Matrix4 {
  const w = (i: number): number => world[i] ?? 0;
  out.elements = [
    w(0),
    w(1),
    -w(2),
    0,
    w(3),
    w(4),
    -w(5),
    0,
    -w(6),
    -w(7),
    w(8),
    0,
    w(9),
    w(10),
    -w(11),
    1,
  ];
  return out;
}

export class SfxMeshes implements MeshPartHandler {
  private readonly slots = new Map<SfxModel, Map<number, Slot>>();
  private readonly mat = new Matrix4();
  private white: DataTexture | null = null;
  private placedCount = 0;
  private disposed = false;
  /** The reach of every model that resolved (or failed: 0) from its pivot, by VFS path (#018 B9). */
  private readonly extents = new Map<string, number>();

  constructor(private readonly deps: SfxMeshesDeps) {}

  /**
   * How far the model at `path` reaches from its pivot (`|centre| + |size| / 2` of its header box, model units), for the culling
   * sphere of the effect (`SfxSystem`). `undefined` until the model has resolved; `0` for a model that does not exist or cannot
   * be decoded (it draws nothing).
   */
  extentOf(path: string): number | undefined {
    return this.extents.get(path);
  }

  /** Counters (probe, tests). */
  get stats(): SfxMeshStats {
    let slots = 0;
    let loading = 0;
    let failed = 0;
    for (const per of this.slots.values()) {
      for (const s of per.values()) {
        slots++;
        if (s.state === 'loading' || s.texturesPending) loading++;
        else if (s.state === 'failed') failed++;
      }
    }
    return { slots, loading, failed, placed: this.placedCount };
  }

  /** A new effect model: starts loading the models of its MESH parts (one hidden instance per part). */
  prepare(model: SfxModel): void {
    if (this.disposed) return;
    for (const part of model.proto.parts) {
      const path = model.meshPathOf(part);
      if (part.kind === 'mesh' && part.inUse && path !== null) {
        this.slotFor(model, part.index, path, part.additive);
      }
    }
  }

  begin(): void {
    this.placedCount = 0;
    for (const per of this.slots.values()) for (const s of per.values()) s.placed = false;
  }

  place(item: MeshItem, renderOrder: number): void {
    if (this.disposed) return;
    const path = item.model.meshPathOf(item.part);
    if (path === null) return;
    const slot = this.slotFor(item.model, item.part.index, path, item.part.additive);
    slot.placed = true;
    slot.idle = 0;
    const inst = slot.inst;
    if (!inst) return;
    this.placedCount++;
    inst.root.visible = true;
    inst.setTransform(partMatrix(item.world, this.mat));
    const model = inst.model;
    if (model.maxFrame > 0) {
      inst.setNodeFrame(meshFrame(item.model.ticks, model.perSlerp, model.maxFrame));
    }
    const a = (item.alpha & 0xff) / 255;
    for (const b of inst.blocks) {
      b.mesh.renderOrder = renderOrder;
      if (slot.glow) {
        const u = (b.mesh.material as ModelMaterial).uniforms['uAlpha'];
        if (u) u.value = a;
      }
    }
  }

  end(): void {
    for (const [model, per] of this.slots) {
      for (const [index, s] of per) {
        if (s.placed) continue;
        if (s.inst) s.inst.root.visible = false;
        if (++s.idle > STALE_FRAMES) {
          this.drop(s);
          per.delete(index);
        }
      }
      if (per.size === 0) this.slots.delete(model);
    }
  }

  /** Releases every instance (renderer disposal, world change). */
  dispose(): void {
    this.disposed = true;
    for (const per of this.slots.values()) for (const s of per.values()) this.drop(s);
    this.slots.clear();
    this.white?.dispose();
    this.white = null;
  }

  // ---- slots -------------------------------------------------------------------------------------------------------

  private slotFor(model: SfxModel, index: number, path: string, glow: boolean): Slot {
    let per = this.slots.get(model);
    if (!per) {
      per = new Map();
      this.slots.set(model, per);
    }
    let slot = per.get(index);
    if (slot) return slot;
    slot = {
      path,
      glow,
      inst: null,
      state: 'loading',
      texturesPending: false,
      placed: false,
      idle: 0,
      dead: false,
    };
    per.set(index, slot);
    this.load(slot);
    return slot;
  }

  private load(slot: Slot): void {
    const fail = (reason: string): void => {
      slot.state = 'failed';
      this.deps.log?.(`sfx mesh ${slot.path}: ${reason}`);
    };
    this.deps.models.model(slot.path, Prio.Fx).then(
      (geometry) => {
        if (slot.dead || this.disposed) return;
        if (!geometry) {
          this.extents.set(slot.path, 0);
          fail('the model does not exist');
          return;
        }
        this.extents.set(slot.path, extentOfBox(geometry.bbox));
        try {
          const options: ModelInstanceOptions = slot.glow
            ? {
                // an effect mesh takes no fog and no model blend
                fog: 'off',
                materialFor: (key, map, alpha) =>
                  createGlowMaterial(key, map ?? this.whiteTexture(), alpha),
              }
            : { fog: 'off' };
          const inst = this.deps.host.createInstance(geometry, options, this.deps.parent);
          inst.root.visible = false;
          slot.inst = inst;
          slot.state = 'ready';
          if (inst.ready) {
            slot.texturesPending = true;
            void inst.ready.then(() => {
              slot.texturesPending = false;
            });
          }
        } catch (e) {
          fail(e instanceof Error ? e.message : String(e));
        }
      },
      (e: unknown) => {
        if (slot.dead) return;
        this.extents.set(slot.path, 0);
        fail(e instanceof Error ? e.message : String(e));
      },
    );
  }

  private drop(slot: Slot): void {
    slot.dead = true;
    if (slot.inst) this.deps.host.removeInstance(slot.inst);
    slot.inst = null;
  }

  private whiteTexture(): Texture {
    if (!this.white) {
      this.white = new DataTexture(
        new Uint8Array([255, 255, 255, 255]),
        1,
        1,
        RGBAFormat,
        UnsignedByteType,
      );
      this.white.needsUpdate = true;
    }
    return this.white;
  }
}
