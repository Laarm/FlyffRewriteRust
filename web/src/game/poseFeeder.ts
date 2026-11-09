// PoseFeeder (Plan/40 section 7.4 "pose assets", WP-CLIENT-18): loads the skeletons and clips the sim needs and forwards the
// `FSK1` / `FMO1` blobs of the `chr` / `ani` worker jobs UNCHANGED to `SimWorld.add_skeleton` / `add_clip` through the
// bridge's load queue (cap L).
//
//  * Skeletons are requested when a mover is added (`skeletonId(path)` hands out the `skel_id` of a `.chr` path, one id per
//    distinct file for the page lifetime) and every skeleton is added once.
//  * Clips are requested lazily: after each iteration with a fresh view `scan` looks at the VISIBLE movers whose `clip_id`
//    (render view slot 28) is not in the sim yet and requests the `.ani` (priority Visible, the own player Critical). A missing
//    clip (`clip = -1`, 449 references, KQ-02) or file is never requested twice and never an error.
//  * The own player's skeleton and STAND clip are requested at Critical by `watchOwn`; the loading gate waits for them.
//
// The asset handles are released as soon as the bytes are queued: the sim copies them, and the decoded asset stays in the
// manager's cache for as long as the renderer holds the same asset (the `chr` type is shared with 41's bind matrices).

import type { AssetHandle } from '../assets/handles';
import type { AssetManager } from '../assets/manager';
import { AniPackType, ChrPackType, type PackValue } from '../assets/poseTypes';
import type { AssetType } from '../assets/types';
import type { ClientDataIndex } from '../data/clientData';
import { OT_MOVER } from '../gen/defs/define';
import { MTI_STAND } from '../gen/defs/neuz';
import { RENDER_VIEW } from '../gen/sim/wire';
import { Prio } from '../workers/priority';
import { createMotionLookup, type MotionLookup } from './motionLookup';
import type { PoseFeederApi, SimBridge, SimLoad } from './ports';
import type { RenderViewReader } from './renderView';

const MV = RENDER_VIEW.mover;
const FLAGS = RENDER_VIEW.moverFlags;

export type PackState = 'loading' | 'queued' | 'ready' | 'missing' | 'failed';

interface Tracked {
  state: PackState;
  readonly path: string;
}

export interface PoseFeederDeps {
  readonly sim: Pick<SimBridge, 'enqueueLoad'>;
  readonly assets: Pick<AssetManager, 'acquire'>;
  /** The client data (the `motions` bundle); `null` before the pre-login bundles are in. */
  readonly data: () => Pick<ClientDataIndex, 'bundle'> | null;
  /** The own player's object id (`0` = none). */
  readonly selfId: () => number;
  readonly log?: { warn(scope: string, message: string): void };
  /** Test seams: the asset types (default: the canonical `chr` / `ani` types of `assets/poseTypes`). */
  readonly types?: {
    readonly chr: AssetType<PackValue<unknown>>;
    readonly ani: AssetType<PackValue<unknown>>;
  };
}

export interface PoseFeederStats {
  readonly skeletons: number;
  readonly skeletonsReady: number;
  readonly clipsRequested: number;
  readonly clipsReady: number;
  readonly clipsMissing: number;
}

export class PoseFeeder implements PoseFeederApi {
  private nextSkel = 1;
  private readonly skeletons = new Map<string, Tracked & { readonly id: number }>();
  private readonly clips = new Map<number, Tracked>();
  private lookup: MotionLookup | null = null;
  private lookupFor: unknown = null;
  private readonly chrType: AssetType<PackValue<unknown>>;
  private readonly aniType: AssetType<PackValue<unknown>>;
  private own: { readonly skel: string; readonly clip: number | null } | null = null;
  private readonly visit = (index: number, id: number, flags: number): void => {
    this.visitMover(index, id, flags);
  };
  private scanView: RenderViewReader | null = null;
  private scanSelf = 0;

  constructor(private readonly deps: PoseFeederDeps) {
    this.chrType = deps.types?.chr ?? ChrPackType;
    this.aniType = deps.types?.ani ?? AniPackType;
  }

  // ---- skeletons ----------------------------------------------------------------------------------------------

  /** The `skel_id` of a `.chr` path; the first call requests the file (Critical for the own player's, else Visible). */
  skeletonId(path: string, prio: Prio = Prio.Visible): number {
    const key = path.toLowerCase();
    const known = this.skeletons.get(key);
    if (known) return known.id;
    const entry = { id: this.nextSkel++, path: key, state: 'loading' as PackState };
    this.skeletons.set(key, entry);
    this.fetchPack(
      this.chrType,
      key,
      prio,
      (blob) => {
        entry.state = 'queued';
        this.deps.sim.enqueueLoad({ kind: 'skeleton', id: entry.id, bytes: blob, prio });
      },
      (state) => {
        entry.state = state;
      },
    );
    return entry.id;
  }

  // ---- clips --------------------------------------------------------------------------------------------------

  private motions(): MotionLookup | null {
    const bundle = this.deps.data()?.bundle('motions');
    if (!bundle) return null;
    if (this.lookupFor !== bundle) {
      this.lookup = createMotionLookup(bundle);
      this.lookupFor = bundle;
    }
    return this.lookup;
  }

  /** Requests clip `clip` (a `ClipId` of the motion table) once. */
  requestClip(clip: number, prio: Prio = Prio.Visible): void {
    if (clip < 0 || clip === 0xffff_ffff || this.clips.has(clip)) return;
    const motions = this.motions();
    if (!motions) return; // the bundle is not here yet: the next scan asks again
    const file = motions.clipFile(clip);
    if (!file?.toLowerCase().endsWith('.ani')) {
      // outside the table, or a built-in `.o3d` animation (the o3d job's FMO1 section is the decor path, 31 13.6)
      this.clips.set(clip, { state: 'missing', path: file ?? '' });
      return;
    }
    const entry: Tracked = { state: 'loading', path: file };
    this.clips.set(clip, entry);
    this.fetchPack(
      this.aniType,
      file,
      prio,
      (blob) => {
        entry.state = 'queued';
        this.deps.sim.enqueueLoad({ kind: 'clip', id: clip, bytes: blob, prio });
      },
      (state) => {
        entry.state = state;
      },
    );
  }

  /** Requests the clip that plays motion `mti` of mover model `moverIndex`. */
  requestMotion(moverIndex: number, mti: number, prio: Prio = Prio.Visible): number | null {
    const clip = this.motions()?.resolveClip(OT_MOVER, moverIndex, mti) ?? null;
    if (clip !== null) this.requestClip(clip, prio);
    return clip;
  }

  private fetchPack(
    type: AssetType<PackValue<unknown>>,
    path: string,
    prio: Prio,
    onBlob: (blob: Uint8Array) => void,
    onState: (state: PackState) => void,
  ): void {
    let handle: AssetHandle<PackValue<unknown>>;
    try {
      handle = this.deps.assets.acquire(type, { ns: 'res', path }, prio);
    } catch (e) {
      onState('failed');
      this.deps.log?.warn('pose', `${path}: ${e instanceof Error ? e.message : String(e)}`);
      return;
    }
    handle.ready.then(
      (value) => {
        try {
          onBlob(value.blob);
        } finally {
          handle.release(); // the sim copies the bytes
        }
      },
      (error: unknown) => {
        handle.release();
        const missing = (error as { missing?: unknown }).missing === true;
        onState(missing ? 'missing' : 'failed');
        // a missing file is invisible by design (KQ-02/KQ-30): no log
        if (!missing) {
          this.deps.log?.warn(
            'pose',
            `${path}: ${error instanceof Error ? error.message : String(error)}`,
          );
        }
      },
    );
  }

  /** The bridge reports each applied (or rejected) load: `queued` becomes `ready`. */
  onSimLoad(load: SimLoad, error: unknown): void {
    if (load.kind === 'skeleton') {
      for (const s of this.skeletons.values()) {
        if (s.id === load.id) s.state = error ? 'failed' : 'ready';
      }
    } else if (load.kind === 'clip') {
      const c = this.clips.get(load.id);
      if (c) c.state = error ? 'failed' : 'ready';
    }
  }

  // ---- the own player and the loading gate ---------------------------------------------------------------------

  /** Requests the own skeleton and STAND clip at Critical (the loading gate waits for them). */
  watchOwn(skeletonPath: string, moverIndex: number): void {
    this.skeletonId(skeletonPath, Prio.Critical);
    const clip = this.requestMotion(moverIndex, MTI_STAND, Prio.Critical);
    this.own = { skel: skeletonPath.toLowerCase(), clip };
  }

  /** Forgets the watched own player (a world change; the sim assets stay). */
  unwatchOwn(): void {
    this.own = null;
  }

  /**
   * `true` when nothing of the own model is still loading: the skeleton and the STAND clip are in the sim (or missing, which
   * never blocks, KQ-02). Without a watched player there is nothing to wait for.
   */
  ownReady(): boolean {
    const own = this.own;
    if (!own) return true;
    const done = (s: PackState | undefined): boolean =>
      s === undefined || s === 'ready' || s === 'missing' || s === 'failed';
    const skel = this.skeletons.get(own.skel);
    if (skel && !done(skel.state)) return false;
    if (own.clip !== null) {
      const clip = this.clips.get(own.clip);
      if (clip && !done(clip.state)) return false;
      // the bundle was not there when the own player spawned: the request is pending
      if (!clip && this.motions() !== null) return false;
    }
    return true;
  }

  // ---- the per-iteration scan (loop step 8) --------------------------------------------------------------------

  scan(view: unknown): void {
    const v = view as RenderViewReader | null;
    if (!v?.valid) return;
    this.scanView = v;
    this.scanSelf = this.deps.selfId();
    v.forEachMover(this.visit);
    this.scanView = null;
  }

  private visitMover(index: number, id: number, flags: number): void {
    if ((flags & FLAGS.VISIBLE) === 0 || (flags & FLAGS.PREVIEW) !== 0) return;
    const view = this.scanView;
    if (!view) return;
    const clip = view.moversU32[index * RENDER_VIEW.MOVER_STRIDE + MV.clipId] ?? 0xffff_ffff;
    if (clip === 0xffff_ffff || this.clips.has(clip)) return;
    this.requestClip(clip, id === this.scanSelf ? Prio.Critical : Prio.Visible);
  }

  get stats(): PoseFeederStats {
    let skeletonsReady = 0;
    for (const s of this.skeletons.values()) if (s.state === 'ready') skeletonsReady += 1;
    let clipsReady = 0;
    let clipsMissing = 0;
    for (const c of this.clips.values()) {
      if (c.state === 'ready') clipsReady += 1;
      else if (c.state === 'missing') clipsMissing += 1;
    }
    return {
      skeletons: this.skeletons.size,
      skeletonsReady,
      clipsRequested: this.clips.size,
      clipsReady,
      clipsMissing,
    };
  }
}

export function createPoseFeeder(deps: PoseFeederDeps): PoseFeeder {
  return new PoseFeeder(deps);
}
