// The client view of the motion table (the `motions` bundle, Plan/24 section 10.5 / Plan/31 section 12.1): which clip a
// motion id plays for a registry model. `resolveClip` mirrors `MotionTable::resolve` of `flyff-sim`
// (`CModelMng::MakeMotionName`, ModelMng.cpp:124-155): an out-of-range or NULL motion id maps to `MTI_STAND`'s slot, a
// missing clip gives `null`, an empty slot gives the model's `empty` clip. The simulation does its own resolution
// from the binary table; the renderer needs this one only to FETCH the right `.ani` files before it asks the sim to
// play a motion (`SimWorld.add_clip(clip_id, FMO1)`).

import type { MotionsBundle } from '../../gen/bundles';
import { MTI_STAND } from '../../gen/defs/neuz';

export interface ModelMotions {
  /** `m_nMax` (motion ids at or above it play STAND). */
  readonly max: number;
  /** The clip of an empty slot (-1 = none). */
  readonly empty: number;
  /** The model's built-in `.o3d` animation (-1 = none). */
  readonly builtin: number;
  /** `mti -> clip` (clip -1 = a missing file). */
  readonly slots: ReadonlyMap<number, number>;
}

export interface MotionIndex {
  /** The `.ani` / `.o3d` VFS path of a clip id (`ClipId` = row index), `undefined` outside the table. */
  clipFile(clip: number): string | undefined;
  /** The frame count of a clip. */
  clipFrames(clip: number): number | undefined;
  model(ot: number, index: number): ModelMotions | undefined;
  /** The clip that plays motion `mti` of model `(ot, index)`, `null` when there is none. */
  resolveClip(ot: number, index: number, mti: number): number | null;
  readonly clipCount: number;
}

const col = (names: readonly string[], n: string): number => names.indexOf(n);

/** Indexes a `motions` bundle. */
export function buildMotionIndex(bundle: MotionsBundle): MotionIndex {
  const cc = bundle.clips.columns;
  const iFile = col(cc, 'file');
  const iFrames = col(cc, 'frames');
  const files: string[] = [];
  const frames: number[] = [];
  for (const row of bundle.clips.rows) {
    files.push(typeof row[iFile] === 'string' ? row[iFile] : '');
    frames.push(typeof row[iFrames] === 'number' ? row[iFrames] : 0);
  }
  const mc = bundle.models.columns;
  const iOt = col(mc, 'ot');
  const iIndex = col(mc, 'index');
  const iMax = col(mc, 'max');
  const iEmpty = col(mc, 'empty');
  const iBuiltin = col(mc, 'builtin');
  const iSlots = col(mc, 'slots');
  const models = new Map<string, ModelMotions>();
  for (const row of bundle.models.rows) {
    const slots = new Map<number, number>();
    const raw = row[iSlots];
    if (Array.isArray(raw)) {
      for (const s of raw as unknown[]) {
        if (Array.isArray(s) && typeof s[0] === 'number' && typeof s[1] === 'number') {
          slots.set(s[0], s[1]);
        }
      }
    }
    models.set(`${String(row[iOt])}:${String(row[iIndex])}`, {
      max: typeof row[iMax] === 'number' ? row[iMax] : 0,
      empty: typeof row[iEmpty] === 'number' ? row[iEmpty] : -1,
      builtin: typeof row[iBuiltin] === 'number' ? row[iBuiltin] : -1,
      slots,
    });
  }
  return {
    clipCount: files.length,
    clipFile: (c) => files[c],
    clipFrames: (c) => frames[c],
    model: (ot, index) => models.get(`${String(ot)}:${String(index)}`),
    resolveClip(ot, index, mti) {
      const mm = models.get(`${String(ot)}:${String(index)}`);
      if (!mm) return null;
      const m = mti === 0xffff_ffff || mti < 0 || mti >= mm.max ? MTI_STAND : mti;
      const slot = mm.slots.get(m);
      if (slot === undefined) return mm.empty >= 0 ? mm.empty : null;
      return slot >= 0 ? slot : null;
    },
  };
}
