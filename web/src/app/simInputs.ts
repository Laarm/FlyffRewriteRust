// The binary sim inputs (Plan/40 section 3.1 B8 and 3.5, WP-CLIENT-11): after login the boot fetches `FLMT` (the motion
// table, `/gen/motion-table/<fp>/all.bin`) and `FLMB` (the model bounds, `/gen/model-bounds/<fp>/all.bin`) at priority
// Critical through the asset layer and hands the bytes UNCHANGED to the sim (`SimWorld.load_motion_table` /
// `load_model_bounds`; from M4 `FLYK` through `load_ctrl_models`). No JSON bundle ever goes to the sim: it reads only
// these binary files (Plan/31 section 17, 24 W10). Until the wasm build exports the loaders, or when the asset manifest
// has no such kind, the step logs one line and skips.

import { keyString, type AssetKey } from '../assets/source';
import { Prio } from '../workers/priority';
import type { ClientContext } from './context';

/** What `SimBridge` offers for the session-wide inputs (WP-CLIENT-18 adds these methods; absent before). */
export interface SimInputsSink {
  loadMotionTable?(bytes: ArrayBuffer): void;
  loadModelBounds?(bytes: ArrayBuffer): void;
  loadCtrlModels?(bytes: ArrayBuffer): void;
}

interface InputSpec {
  readonly key: AssetKey;
  readonly method: keyof SimInputsSink;
}

const INPUTS: readonly InputSpec[] = [
  { key: { ns: 'gen', kind: 'motion-table' }, method: 'loadMotionTable' },
  { key: { ns: 'gen', kind: 'model-bounds' }, method: 'loadModelBounds' },
  { key: { ns: 'gen', kind: 'ctrl-models' }, method: 'loadCtrlModels' },
];

export interface SimInputsResult {
  /** The keys that were fetched and handed to the sim. */
  readonly loaded: readonly string[];
  /** The keys that were skipped, with the reason. */
  readonly skipped: readonly { key: string; reason: string }[];
}

/** Fetches the inputs the manifest lists and passes each to the matching sim method. Never throws. */
export async function loadSimInputs(ctx: ClientContext): Promise<SimInputsResult> {
  const sink = ctx.sim as SimInputsSink;
  const loaded: string[] = [];
  const skipped: { key: string; reason: string }[] = [];
  await Promise.all(
    INPUTS.map(async ({ key, method }) => {
      const name = keyString(key);
      if (ctx.assetLayer.source.url(key) === null) {
        skipped.push({ key: name, reason: 'the asset manifest has no such kind' });
        return;
      }
      const load: unknown = Reflect.get(sink, method);
      if (typeof load !== 'function') {
        skipped.push({ key: name, reason: `the sim has no ${method} yet` });
        return;
      }
      try {
        const bytes = await ctx.assets.fetchBytes(key, Prio.Critical);
        (load as (this: unknown, bytes: ArrayBuffer) => void).call(ctx.sim, bytes);
        loaded.push(name);
      } catch (error) {
        const reason = error instanceof Error ? error.message : String(error);
        ctx.log.error('boot', `${name}: ${reason}`);
        skipped.push({ key: name, reason });
      }
    }),
  );
  for (const s of skipped) ctx.log.info('boot', `sim input ${s.key} skipped: ${s.reason}`);
  return { loaded, skipped };
}
