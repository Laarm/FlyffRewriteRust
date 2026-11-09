// Preview movers inside a `SimWorld` for the renderer's own pages and the 3D previews (Plan/41 section 21.1,
// Plan/31 section 15.1 `ADD_MOVER` kind 4): local, non-networked movers with a fixed motion sequence whose pose,
// palette and attachments come out of `render_view` like any other mover's. The class owns one `SimWorld`
// handle, the skeleton / clip ids it loaded and the `SimPoseSource` (palette texture) that follows it; in the game
// the same records go through `SimBridge`'s command buffer (doc 40 section 7.3) and `PreviewMotionSink` is its seam.
//
// Asset flow: `ensureSkeleton(path)` fetches the `FSK1` pack once and `add_skeleton`s it, `ensureMotions` resolves the
// clip of every motion through the `motions` bundle (`MotionIndex`, the quirks of `MakeMotionName`) and `add_clip`s
// the `FMO1` packs it does not have yet. Everything is awaited BEFORE the `ACT` that starts the motion.

import type { SimWorldHandle } from '../../wasm/simTypes';
import { OT_MOVER } from '../../gen/defs/define';
import type { MotionIndex } from './motionIndex';
import type { PaletteHost } from './PaletteTexture';
import { SimPoseSource } from './poseSource';
import {
  addPreviewMover,
  ANILOOP,
  concatCommands,
  previewMotion,
  removeMover,
  type PreviewMoverSpec,
} from './simCommands';

/** Where the packs come from (the worker pool in the browser, the native decoders in tests). */
export interface SimAssetSource {
  /** The `FSK1` pack of a `.chr` VFS path, `null` when the file does not exist. */
  skeletonPack(path: string): Promise<Uint8Array | null>;
  /** The `FMO1` pack of an `.ani` VFS path, `null` when the file does not exist. */
  clipPack(path: string): Promise<Uint8Array | null>;
}

/** What the preview renderer needs to animate previews (implemented by `SimPreviewHost`, later by `SimBridge`). */
export interface PreviewMotionSink {
  /** Adds the preview mover and resolves once its skeleton and the clips of `motions` are in the sim. */
  addPreview(spec: PreviewAddSpec): Promise<void>;
  /** Starts motion `mti` on the preview (with `queue` behind a running 1-play motion). */
  setMotion(id: number, mti: number, loop?: number, queue?: boolean): void;
  removePreview(id: number): void;
}

/** A preview mover to add. */
export interface PreviewAddSpec {
  readonly id: number;
  /** `MoverProp` / registry index (`MI_MALE`, a monster id). */
  readonly moverIndex: number;
  /** The `.chr` of the model. */
  readonly skeleton: string;
  readonly pos?: readonly [number, number, number];
  readonly angle?: number;
  readonly scale?: number;
  /** Motions that will be played (their clips are loaded first). */
  readonly motions?: readonly number[];
}

export class SimPreviewHost implements PreviewMotionSink {
  readonly pose: SimPoseSource;
  private readonly skeletons = new Map<string, Promise<number | null>>();
  private readonly clips = new Map<number, Promise<boolean>>();
  private nextSkel = 1;
  /** `dispose()` freed the sim: asset promises that settle later must not call into it ("moved value"). */
  private disposed = false;
  private readonly movers = new Map<number, number>();

  constructor(
    readonly sim: SimWorldHandle,
    memory: () => WebAssembly.Memory,
    private readonly assets: SimAssetSource,
    private readonly motions: MotionIndex,
    host: PaletteHost,
  ) {
    this.pose = new SimPoseSource(sim, memory, host);
  }

  /** Loads the skeleton once; resolves with its sim id, `null` when it does not exist. */
  ensureSkeleton(path: string): Promise<number | null> {
    let p = this.skeletons.get(path);
    if (!p) {
      const id = this.nextSkel++;
      p = this.assets.skeletonPack(path).then((pack) => {
        if (pack === null || this.disposed) return null;
        this.sim.add_skeleton(id, pack);
        return id;
      });
      this.skeletons.set(path, p);
    }
    return p;
  }

  /** Loads clip `clip` (a `ClipId` of the motion table) once; `false` when its file does not exist. */
  private ensureClip(clip: number): Promise<boolean> {
    let p = this.clips.get(clip);
    if (!p) {
      const file = this.motions.clipFile(clip);
      p = !file?.toLowerCase().endsWith('.ani')
        ? Promise.resolve(false)
        : this.assets.clipPack(file).then((pack) => {
            if (pack === null || this.disposed) return false;
            this.sim.add_clip(clip, pack);
            return true;
          });
      this.clips.set(clip, p);
    }
    return p;
  }

  /** Loads the clips that playing `mtis` on model `(OT_MOVER, index)` needs (each motion plus STAND, the fallback). */
  async ensureMotions(index: number, mtis: readonly number[]): Promise<void> {
    const wanted = new Set<number>();
    for (const m of [0, ...mtis]) {
      const c = this.motions.resolveClip(OT_MOVER, index, m);
      if (c !== null) wanted.add(c);
    }
    await Promise.all([...wanted].map((c) => this.ensureClip(c)));
  }

  async addPreview(spec: PreviewAddSpec): Promise<void> {
    const skel = await this.ensureSkeleton(spec.skeleton);
    await this.ensureMotions(spec.moverIndex, spec.motions ?? []);
    if (this.disposed) return;
    const s: PreviewMoverSpec = {
      id: spec.id,
      moverIndex: spec.moverIndex,
      modelType: OT_MOVER,
      modelIndex: spec.moverIndex,
      skelId: skel ?? 0,
      pos: spec.pos ?? [0, 0, 0],
      angle: spec.angle ?? 0,
      scale: spec.scale ?? 1,
    };
    this.sim.apply_commands(addPreviewMover(s));
    this.movers.set(spec.id, spec.moverIndex);
  }

  setMotion(id: number, mti: number, loop: number = ANILOOP.LOOP, queue = false): void {
    if (this.disposed) return;
    this.sim.apply_commands(previewMotion(id, mti, loop, 0, queue));
  }

  /** `SET_MOTION` of several records in one call. */
  setMotions(id: number, seq: readonly { mti: number; loop: number }[]): void {
    if (this.disposed) return;
    this.sim.apply_commands(
      concatCommands(seq.map((m, i) => previewMotion(id, m.mti, m.loop, 0, i > 0))),
    );
  }

  removePreview(id: number): void {
    this.movers.delete(id);
    if (this.disposed) return;
    this.sim.apply_commands(removeMover(id));
  }

  /** `step(frames)` of the sim (60 Hz frames); returns the frames stepped. */
  step(frames: number): number {
    if (this.disposed) return 0;
    return this.sim.step(frames);
  }

  /** Calls `render_view(alpha)` and uploads the palette. */
  update(alpha = 1): void {
    if (this.disposed) return;
    this.pose.update(alpha);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.pose.dispose();
    this.sim.free();
  }
}
