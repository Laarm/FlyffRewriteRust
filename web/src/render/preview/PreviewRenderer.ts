// 3D previews rendered into DOM canvases (Plan/41 section 21, R12, WP-REND-06; F-317/F-342 render parts): the
// character-select slots, the character creation window, the paper doll and portrait, and the model viewer.
//
//  * every preview owns a `Scene` with ONE `MoverVisual` (or an item `ModelInstance`) and a `WebGLRenderTarget` of the
//    canvas size in the MAIN context (no second WebGL context, no `preserveDrawingBuffer`),
//  * it is rendered at most `maxFps` (default 15) times per second when `animate` is set, once (and again on
//    `invalidate`) otherwise, with the preview environment: lighting off, white diffuse/ambient, no fog, hair ambient =
//    hair colour (`presets.ts`),
//  * the pixels come back through an ASYNCHRONOUS read-back (`readRenderTargetPixelsAsync`: PBO + fence, one frame of
//    latency; never a synchronous `readPixels`) and are put into the DOM canvas with `putImageData`; at most one
//    read-back per preview is in flight, so a slow GPU lowers the rate instead of queueing work,
//  * animation: the pose of a preview comes from a preview mover of the `SimWorld` (`ADD_MOVER` kind 4, motions by id
//    through the command buffer, Plan/31 section 15.1) when a `PreviewMotionSink` is given; without one (M1) the preview
//    is drawn in the BIND pose (identity palette, attachments from the `FSK1` bind matrices) and the requested motion
//    ids are still tracked for the probe.
//
// The caller steps the simulation and updates the pose source (`SimPreviewHost.step/update`, or the game loop) before
// calling `render(nowMs)`; previews only read poses.

import {
  Box3,
  Matrix4,
  NearestFilter,
  NoColorSpace,
  RGBAFormat,
  Scene,
  UnsignedByteType,
  Vector3,
  WebGLRenderTarget,
  type Object3D,
  type PerspectiveCamera,
} from 'three';
import type { MoverVisualState } from '../../game/entities';
import { MI_FEMALE, MI_MALE } from '../../gen/defs/obj';
import { RenderCamera } from '../camera';
import type { EnvLight } from '../gl/materials/env';
import type { ModelGeometry } from '../objects/ModelCache';
import type { ModelInstance } from '../objects/ModelInstance';
import type { WorldRenderer } from '../WorldRenderer';
import { mirrorVec3 } from '../space';
import type { CameraState, Vec3LH } from '../types';
import { visualHash } from '../movers/MoverBatcher';
import { ANILOOP } from '../movers/simCommands';
import {
  MoverVisual,
  resolveVisualPlan,
  type ModelProvider,
  type VisualHost,
  type VisualRegistryLike,
} from '../movers/MoverVisual';
import type { PreviewMotionSink } from '../movers/SimPreviewHost';
import type { BindPoseSource, PoseSource } from '../movers/poseSource';
import {
  MAX_PREVIEW_FPS,
  PREVIEW_ENV,
  presetCameraTurned,
  presetModelMatrixThree,
  resolvePreset,
  previewMotions,
  type PreviewMotion,
  type PreviewPresetName,
} from './presets';

// ---- the backend (GL part) --------------------------------------------------------------------------------

export interface PreviewTarget {
  readonly width: number;
  readonly height: number;
  dispose(): void;
}

/** The GL operations of a preview (`WebGlPreviewBackend` for the renderer; a fake in the unit tests). */
export interface PreviewBackend {
  createTarget(width: number, height: number): PreviewTarget;
  /** Renders `scene` with `camera` into `target` (transparent clear) using the preview environment. */
  render(target: PreviewTarget, scene: Object3D, camera: PerspectiveCamera, env: EnvLight): void;
  /** Asynchronous read-back of the whole target: RGBA bytes, rows bottom-up (the GL order). */
  readAsync(target: PreviewTarget): Promise<Uint8Array>;
}

/** The backend over the main renderer: render targets of the main context, `readRenderTargetPixelsAsync`. */
export function createWebGlPreviewBackend(renderer: WorldRenderer): PreviewBackend {
  interface GlTarget extends PreviewTarget {
    readonly rt: WebGLRenderTarget;
  }
  const gl = renderer.device.renderer;
  return {
    createTarget(width, height): GlTarget {
      const rt = new WebGLRenderTarget(width, height, {
        format: RGBAFormat,
        type: UnsignedByteType,
        minFilter: NearestFilter,
        magFilter: NearestFilter,
        depthBuffer: true,
        stencilBuffer: false,
        generateMipmaps: false,
      });
      rt.texture.colorSpace = NoColorSpace;
      return {
        width,
        height,
        rt,
        dispose: () => {
          rt.dispose();
        },
      };
    },
    render(target, scene, camera, env) {
      const rt = (target as GlTarget).rt;
      renderer.materials.setEnvironment(env);
      const previous = gl.getRenderTarget();
      gl.setRenderTarget(rt);
      gl.setClearColor(0x000000, 0);
      gl.clear(true, true, false);
      camera.layers.enableAll();
      gl.sortObjects = true;
      gl.render(scene, camera);
      gl.setRenderTarget(previous);
    },
    async readAsync(target) {
      const rt = (target as GlTarget).rt;
      const buffer = new Uint8Array(rt.width * rt.height * 4);
      await gl.readRenderTargetPixelsAsync(rt, 0, 0, rt.width, rt.height, buffer);
      return buffer;
    },
  };
}

export interface PreviewCanvasLike {
  width: number;
  height: number;
  getContext(kind: '2d'): { putImageData(image: ImageData, x: number, y: number): void } | null;
}

/** What a window gives `open`. */
export interface PreviewSlot {
  /** The DOM canvas inside a window (42/45). */
  readonly canvas: PreviewCanvasLike;
  readonly preset: PreviewPresetName;
  /** The character (or NPC) to show, or an item model for the model viewer. */
  readonly subject: MoverVisualState | { readonly itemModel: number };
  /** Redraw continuously (animated); else once and on `invalidate`. */
  readonly animate: boolean;
  /** Requested rate, clamped to `maxFps` (15). */
  readonly fps?: number;
  /** Character-select: this slot is the selected one (gets up and runs; the others sit). */
  readonly selected?: boolean;
}

export interface PreviewHandle {
  readonly id: number;
  /** Index of the slot in open order (the probe's `slot`). */
  readonly index: number;
  readonly preset: PreviewPresetName;
  /** The motion ids requested of the simulation, in order (probe). */
  readonly motions: readonly number[];
  /** Non-transparent pixels of the last frame put into the canvas. */
  readonly nonEmptyPixels: number;
  /** Frames put into the canvas. */
  readonly framesPresented: number;
  /** Settles when the models and textures of the preview are resident. */
  readonly ready: Promise<void>;
  /** World positions (three.js space) of the loaded parts by `PARTS_*` slot (weapon placement checks, probes). */
  partPositions(): { slot: number; position: [number, number, number] }[];
  setSelected(selected: boolean): void;
  setSubject(subject: MoverVisualState): void;
  /** Orbit of the `modelView` preset: yaw/pitch in degrees, zoom factor. */
  setOrbit(yawDeg: number, pitchDeg: number, zoom: number): void;
  /** The extra `RotY` of the model in degrees (`m_fRot` of the paper doll, dragged with the pointer); `0` at the start. */
  setRotation(degrees: number): void;
  invalidate(): void;
}

export interface PreviewProbeRow {
  readonly slot: number;
  readonly preset: PreviewPresetName;
  readonly motion: number;
  readonly motions: readonly number[];
  readonly nonEmptyPixels: number;
}

export interface PreviewRendererDeps {
  readonly backend: PreviewBackend;
  readonly host: VisualHost;
  readonly models: ModelProvider;
  readonly registry: VisualRegistryLike;
  /** The pose source previews follow (the sink's sim, or the bind-pose source). */
  readonly pose: PoseSource;
  /** Animation through the simulation; without it previews are drawn in the bind pose. */
  readonly sink?: PreviewMotionSink;
  /** The bind-pose source (the `pose` itself) when there is no sink. */
  readonly bind?: BindPoseSource;
  readonly maxFps?: number;
  /** `features.run_on_charselect`. */
  readonly runOnCharSelect?: boolean;
  /** `new ImageData(data, w, h)` (injected for tests). */
  readonly imageData?: (
    data: Uint8ClampedArray<ArrayBuffer>,
    width: number,
    height: number,
  ) => ImageData;
}

/** The ids of preview movers in the pose source: far above any `ObjId` of a session. */
export const PREVIEW_ID_BASE = 0x7ff0_0000;

interface Entry {
  readonly h: HandleImpl;
  closed: boolean;
}

class HandleImpl implements PreviewHandle {
  readonly scene = new Scene();
  readonly cam = new RenderCamera();
  target: PreviewTarget | null = null;
  visual: MoverVisual | null = null;
  item: ModelInstance | null = null;
  itemModel: ModelGeometry | null = null;
  motions: number[] = [];
  nonEmptyPixels = 0;
  framesPresented = 0;
  lastRenderMs = Number.NEGATIVE_INFINITY;
  pending = false;
  dirty = true;
  bound = false;
  orbit = { yaw: 0, pitch: 10, zoom: 1 };
  rotation = 0;
  /** `visualHash` of the subject the visual was last assembled for: a state edited in place (an equip) re-assembles it. */
  hash = 0;
  ready: Promise<void> = Promise.resolve();
  subject: PreviewSlot['subject'];
  selected: boolean;

  constructor(
    readonly id: number,
    readonly index: number,
    readonly slot: PreviewSlot,
    private readonly owner: PreviewRenderer,
  ) {
    this.subject = slot.subject;
    this.selected = slot.selected === true;
    this.scene.matrixWorldAutoUpdate = false;
  }

  get preset(): PreviewPresetName {
    return this.slot.preset;
  }

  partPositions(): { slot: number; position: [number, number, number] }[] {
    const v = this.visual;
    if (!v) return [];
    const out: { slot: number; position: [number, number, number] }[] = [];
    for (const slot of v.loadedSlots()) {
      const position = v.partPosition(slot);
      if (position) out.push({ slot, position });
    }
    return out;
  }

  setSelected(selected: boolean): void {
    if (this.selected === selected) return;
    this.selected = selected;
    this.owner.requestMotions(this);
    this.dirty = true;
  }

  setSubject(subject: MoverVisualState): void {
    this.subject = subject;
    this.hash = visualHash(subject);
    this.visual?.setState(subject);
    this.dirty = true;
    this.redrawWhenLoaded();
  }

  /**
   * A new look needs models and textures that may not be resident: a still preview (`animate: false`) is drawn once when the state changes, so it
   * must be drawn again when those loads settle (the beauty shops' head cards page through heads that were never fetched).
   */
  redrawWhenLoaded(): void {
    const v = this.visual;
    if (!v) return;
    void v.ready.then(() => {
      this.dirty = true;
    });
  }

  setOrbit(yawDeg: number, pitchDeg: number, zoom: number): void {
    this.orbit = { yaw: yawDeg, pitch: pitchDeg, zoom };
    this.dirty = true;
  }

  setRotation(degrees: number): void {
    if (this.rotation === degrees) return;
    this.rotation = degrees;
    this.dirty = true;
  }

  invalidate(): void {
    this.dirty = true;
  }
}

/** A mover state is a character (humanoid), not an item model. */
function isMover(s: PreviewSlot['subject']): s is MoverVisualState {
  return 'kind' in s;
}

const IDENTITY_FRAME_INPUT = { objectDetail: 0 as const, distCamera: 3.5 };

export class PreviewRenderer {
  private readonly entries: Entry[] = [];
  private nextId = PREVIEW_ID_BASE;
  private nextIndex = 0;
  private readonly maxFps: number;

  constructor(private readonly d: PreviewRendererDeps) {
    this.maxFps = Math.min(MAX_PREVIEW_FPS, Math.max(1, d.maxFps ?? MAX_PREVIEW_FPS));
  }

  /** The number of open previews. */
  get count(): number {
    return this.entries.length;
  }

  // ---- open / close ---------------------------------------------------------------------------------------

  open(slot: PreviewSlot): PreviewHandle {
    const h = new HandleImpl(this.nextId++, this.nextIndex++, slot, this);
    this.entries.push({ h, closed: false });
    const loading: Promise<unknown>[] = [];
    if (isMover(slot.subject)) {
      const state = slot.subject;
      const visual = new MoverVisual(h.id, state, {
        pose: this.d.pose,
        parent: h.scene,
        host: this.d.host,
        models: this.d.models,
        registry: this.d.registry,
        lighting: false,
      });
      h.visual = visual;
      h.hash = visualHash(state);
      loading.push(visual.ready);
      const plan = resolveVisualPlan(state, this.d.registry, { activeMover: false });
      const preset = resolvePreset(slot.preset, state.sex);
      const seq = previewMotions(
        slot.preset,
        h.selected,
        this.d.runOnCharSelect !== false,
        state.sex,
      );
      h.motions = seq.map((m) => m.mti);
      if (this.d.sink && plan.skeleton !== null) {
        const index = plan.ifParts ? (state.sex === 0 ? MI_MALE : MI_FEMALE) : state.moverIndex;
        loading.push(
          this.d.sink
            .addPreview({
              id: h.id,
              moverIndex: index,
              skeleton: plan.skeleton,
              pos: [preset.translateX ?? 0, preset.translateY, preset.translateZ ?? 0],
              // the sim turns a mover by `RotY(-angle)` (`CObj::UpdateMatrix`), the preset states the D3DX `RotY` itself
              angle: -(preset.rotYDeg ?? 0),
              scale: preset.scale,
              motions: h.motions,
            })
            .then(() => {
              this.sendMotions(h, seq);
            }),
        );
      }
    } else {
      loading.push(this.openItem(h, slot.subject.itemModel));
    }
    h.ready = Promise.all(loading).then(() => {
      h.dirty = true;
    });
    return h;
  }

  /** The item model viewer: the model of an item, centred on its bounds. */
  private async openItem(h: HandleImpl, itemId: number): Promise<void> {
    const entry = this.d.registry.itemModel?.(itemId);
    const file = entry?.file ?? null;
    if (file === null) return;
    const model = await this.d.models.model(file);
    if (model === null) return;
    h.itemModel = model;
    h.item = this.d.host.createInstance(model, { rules: {} }, h.scene);
    await h.item.ready;
  }

  close(handle: PreviewHandle): void {
    const e = this.entries.find((x) => x.h === handle);
    if (!e || e.closed) return;
    e.closed = true;
    this.entries.splice(this.entries.indexOf(e), 1);
    const h = e.h;
    h.visual?.dispose();
    if (h.item) this.d.host.removeInstance(h.item);
    h.target?.dispose();
    if (this.d.sink) this.d.sink.removePreview(h.id);
    this.d.bind?.remove(h.id);
  }

  // ---- motions ---------------------------------------------------------------------------------------------

  /** (Re)sends the motion sequence of a handle to the simulation (`MTI_GETUP` then `MTI_RUN`, ...). */
  requestMotions(h: HandleImpl): void {
    const seq = previewMotions(
      h.preset,
      h.selected,
      this.d.runOnCharSelect !== false,
      isMover(h.subject) ? h.subject.sex : 0,
    );
    h.motions = seq.map((m) => m.mti);
    this.sendMotions(h, seq);
  }

  private sendMotions(h: HandleImpl, seq: readonly PreviewMotion[]): void {
    const sink = this.d.sink;
    if (!sink) return;
    seq.forEach((m, i) => {
      sink.setMotion(h.id, m.mti, m.loop === 1 ? ANILOOP.PLAY1 : ANILOOP.LOOP, i > 0);
    });
  }

  // ---- per frame ---------------------------------------------------------------------------------------------

  /**
   * Renders every preview that is due and starts its asynchronous read-back. Call once per frame (or per timer tick),
   * after the pose source was updated for this frame.
   */
  render(nowMs: number): void {
    for (const { h } of this.entries) {
      const fps = Math.min(this.maxFps, h.slot.fps ?? this.maxFps);
      const interval = 1000 / fps;
      const due = h.slot.animate ? nowMs - h.lastRenderMs >= interval : h.dirty;
      if (!due || h.pending) continue;
      this.renderOne(h, nowMs);
    }
  }

  private renderOne(h: HandleImpl, nowMs: number): void {
    const canvas = h.slot.canvas;
    const w = Math.max(1, Math.trunc(canvas.width));
    const hgt = Math.max(1, Math.trunc(canvas.height));
    if (h.target?.width !== w || h.target.height !== hgt) {
      h.target?.dispose();
      h.target = this.d.backend.createTarget(w, hgt);
    }
    const preset = resolvePreset(h.preset, isMover(h.subject) ? h.subject.sex : 0);

    if (h.visual) {
      // the M1 bind pose: register the skeleton once it is resident and place the model
      if (!this.d.sink && this.d.bind && !h.bound && h.visual.skeleton) {
        this.d.bind.add(h.id, h.visual.skeleton);
        this.d.bind.setWorld(h.id, new Matrix4().fromArray(presetModelMatrixThree(preset)));
        h.bound = true;
      }
      // the own character's state is edited in place by the equip messages: the paper doll follows it
      if (isMover(h.subject)) {
        const hash = visualHash(h.subject);
        if (hash !== h.hash) {
          h.hash = hash;
          h.visual.setState(h.subject);
          h.redrawWhenLoaded();
        }
      }
      h.visual.update({ nowMs, ...IDENTITY_FRAME_INPUT });
      h.cam.update(presetCameraTurned(preset, h.rotation), w / hgt);
    } else if (h.item && h.itemModel) {
      h.cam.update(this.itemCamera(h, h.itemModel), w / hgt);
    }
    h.scene.updateMatrixWorld(true);
    this.d.backend.render(h.target, h.scene, h.cam.camera, PREVIEW_ENV);
    h.lastRenderMs = nowMs;
    h.dirty = false;
    h.pending = true;
    const target = h.target;
    void this.d.backend.readAsync(target).then(
      (px) => {
        h.pending = false;
        if (this.entries.some((e) => e.h === h)) this.present(h, px, target);
      },
      () => {
        h.pending = false;
      },
    );
  }

  /** The camera of the item viewer: the model's bounds centred, the orbit of the window. */
  private itemCamera(h: HandleImpl, model: ModelGeometry): CameraState {
    const box = new Box3().copy(model.bbox);
    const centre = box.getCenter(new Vector3());
    const radius = Math.max(0.05, box.getSize(new Vector3()).length() / 2);
    const dist = (radius / Math.tan(Math.PI / 8)) * 1.15 * h.orbit.zoom;
    const yaw = (h.orbit.yaw * Math.PI) / 180;
    const pitch = (h.orbit.pitch * Math.PI) / 180;
    const c: Vec3LH = mirrorVec3([centre.x, centre.y, centre.z]);
    return {
      pos: [
        c[0] + dist * Math.cos(pitch) * Math.sin(yaw),
        c[1] + dist * Math.sin(pitch),
        c[2] - dist * Math.cos(pitch) * Math.cos(yaw),
      ],
      lookAt: c,
      up: [0, 1, 0],
      fovDiv: 4,
      telescope: 0,
      yawDeg: 0,
      mode: 'normal',
    };
  }

  /** Puts the read-back pixels (bottom-up) into the DOM canvas and counts the drawn pixels. */
  private present(h: HandleImpl, px: Uint8Array, target: PreviewTarget): void {
    const canvas = h.slot.canvas;
    const ctx = canvas.getContext('2d');
    const { width, height } = target;
    if (px.length < width * height * 4) return;
    const out = new Uint8ClampedArray(new ArrayBuffer(width * height * 4));
    let nonEmpty = 0;
    const row = width * 4;
    for (let y = 0; y < height; y++) {
      const src = (height - 1 - y) * row; // GL rows are bottom-up
      out.set(px.subarray(src, src + row), y * row);
    }
    for (let i = 3; i < out.length; i += 4) if ((out[i] ?? 0) > 0) nonEmpty++;
    h.nonEmptyPixels = nonEmpty;
    h.framesPresented++;
    if (ctx) {
      const make = this.d.imageData ?? ((d, w, hh) => new ImageData(d, w, hh));
      ctx.putImageData(make(out, width, height), 0, 0);
    }
  }

  // ---- probe -------------------------------------------------------------------------------------------------

  /** `window.__flyffDebug.render.previews`: `{ slot, preset, motion, nonEmptyPixels }[]` (Plan/41 section 21.3). */
  probe(): PreviewProbeRow[] {
    return this.entries.map(({ h }) => ({
      slot: h.index,
      preset: h.preset,
      motion: h.motions[0] ?? -1,
      motions: [...h.motions],
      nonEmptyPixels: h.nonEmptyPixels,
    }));
  }

  /** Draw calls of the visible previews (the per-mover cost of a preview). */
  drawCalls(): number {
    let n = 0;
    for (const { h } of this.entries) n += h.visual?.drawCalls() ?? 0;
    return n;
  }

  dispose(): void {
    for (const { h } of [...this.entries]) this.close(h);
  }
}
