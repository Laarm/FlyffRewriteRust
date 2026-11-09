// The GPU upload queue and VRAM accounting (Plan/40 section 9.3, 9.6, 12.4; WP-CLIENT-22). The queue itself - budgets of
// 4 MB / 3 ms per frame in game, 16 MB / 12 ms while loading, none while the tab is hidden, the order `Prio` > distance > FIFO and
// the progress guarantee (at least one job per frame) - lives in `render/gl/uploadQueue.ts` because it has no dependency on
// three.js and 41 owns the jobs it runs; this module re-exports it and adds the part that belongs to the loop: the mode of the
// current iteration (`createUploads` is the `UploadsApi` the loop calls at step 10) and the WebGL context-loss coordination
// (`coordinateContextLoss`: the manager marks every GPU-backed handle for re-upload and `gpuBytes` is 0 again).

import type { UploadsApi } from '../game/ports';
import {
  GpuAccounting,
  UPLOAD_BUDGETS,
  UploadQueue,
  type UploadFrameResult,
  type UploadJob,
  type UploadMode,
} from '../render/gl/uploadQueue';
import type { AssetManager } from './manager';

export { GpuAccounting, UPLOAD_BUDGETS, UploadQueue };
export type { UploadFrameResult, UploadJob, UploadMode };

export interface UploadsDeps {
  readonly queue: UploadQueue;
  /** `true` while the screen is `Loading` (nothing is being played: the loading budget applies). */
  readonly loading: () => boolean;
  /** `true` in a hidden tab: no uploads (the loop does not render then, this is a second guard). */
  readonly hidden: () => boolean;
}

/** The loop's upload port plus what the probe reads. */
export interface UploadsController extends UploadsApi {
  /** The mode of the last frame. */
  readonly mode: UploadMode;
  /** Numbers of the last frame (`pending` is what is still queued). */
  readonly last: UploadFrameResult;
  /** Frames that ran at least one job. */
  readonly framesWithJobs: number;
}

/** The mode of an iteration: hidden beats loading beats game. */
export function uploadMode(loading: boolean, hidden: boolean): UploadMode {
  return hidden ? 'hidden' : loading ? 'loading' : 'game';
}

/** The `UploadsApi` of the loop (step 10): picks the budget of the iteration and runs the queue once. */
export function createUploads(deps: UploadsDeps): UploadsController {
  let mode: UploadMode = 'game';
  let framesWithJobs = 0;
  return {
    runFrame() {
      mode = uploadMode(deps.loading(), deps.hidden());
      deps.queue.mode = mode;
      if (deps.queue.runFrame().jobs > 0) framesWithJobs += 1;
    },
    get mode() {
      return mode;
    },
    get last() {
      return deps.queue.last;
    },
    get framesWithJobs() {
      return framesWithJobs;
    },
  };
}

/**
 * WebGL context loss (Plan/40 section 12.4): the manager marks every GPU-backed handle for re-upload and resets its
 * `gpuBytes`, the renderer-side accounting is reset too. Returns the two restore actions: `restored()` queues the uploads of the
 * marked handles again (41 re-creates its own resources the same way, from the HTTP cache).
 */
export function coordinateContextLoss(
  manager: Pick<AssetManager, 'contextLost' | 'contextRestored'>,
  accounting: Pick<GpuAccounting, 'reset'>,
): { lost(): number; restored(): number } {
  return {
    lost() {
      accounting.reset();
      return manager.contextLost();
    },
    restored() {
      return manager.contextRestored();
    },
  };
}
