// Which `.ani` file plays a motion of a registry model (Plan/40 section 7.4 "pose assets", Plan/24 section 10.5): the
// client view of the `motions` bundle. The SIMULATION resolves clips from the binary `FLMT` table; the client needs this
// lookup only to FETCH the right `.ani` files (`SimWorld.add_clip(clip_id, FMO1)`) before the sim asks for them.
// `resolveClip` mirrors `MotionTable::resolve` (`CModelMng::MakeMotionName`): an out-of-range or NULL motion id maps to the
// `MTI_STAND` slot, a missing clip gives `null`, an empty slot gives the model's `empty` clip (the KQ-02 quirk file).

import type { MotionsBundle } from '../gen/bundles';
import { MTI_STAND } from '../gen/defs/neuz';

export interface MotionLookup {
  /** The `.ani` (or `.o3d`) VFS path of a clip id (= row index of the `clips` table); `undefined` outside the table. */
  clipFile(clip: number): string | undefined;
  /** The clip that plays motion `mti` of model `(ot, index)`; `null` when there is none. */
  resolveClip(ot: number, index: number, mti: number): number | null;
  /** The built-in clip of the model's own `.o3d` node animation (ctrls and animated statics), `null` when it has none. */
  builtinClip(ot: number, index: number): number | null;
  readonly clipCount: number;
}

const NULL_MOTION = 0xffff_ffff;

interface ModelMotions {
  readonly max: number;
  readonly empty: number;
  readonly slots: ReadonlyMap<number, number>;
}

function column(names: readonly string[], name: string): number {
  return names.indexOf(name);
}

/** Indexes a `motions` bundle (only the columns the lookup needs). */
export function createMotionLookup(bundle: MotionsBundle): MotionLookup {
  const iFile = column(bundle.clips.columns, 'file');
  const files: string[] = [];
  for (const row of bundle.clips.rows) {
    const f = row[iFile];
    files.push(typeof f === 'string' ? f : '');
  }
  const mc = bundle.models.columns;
  const iOt = column(mc, 'ot');
  const iIndex = column(mc, 'index');
  const iMax = column(mc, 'max');
  const iEmpty = column(mc, 'empty');
  const iSlots = column(mc, 'slots');
  const iBuiltin = column(mc, 'builtin');
  const builtin = new Map<string, number>();
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
    const max = row[iMax];
    const empty = row[iEmpty];
    const b = iBuiltin >= 0 ? row[iBuiltin] : undefined;
    if (typeof b === 'number' && b >= 0)
      builtin.set(`${String(row[iOt])}:${String(row[iIndex])}`, b);
    models.set(`${String(row[iOt])}:${String(row[iIndex])}`, {
      max: typeof max === 'number' ? max : 0,
      empty: typeof empty === 'number' ? empty : -1,
      slots,
    });
  }
  return {
    clipCount: files.length,
    clipFile: (c) => files[c],
    builtinClip: (ot, index) => builtin.get(`${String(ot)}:${String(index)}`) ?? null,
    resolveClip(ot, index, mti) {
      const mm = models.get(`${String(ot)}:${String(index)}`);
      if (!mm) return null;
      const m = mti === NULL_MOTION || mti < 0 || mti >= mm.max ? MTI_STAND : mti;
      const slot = mm.slots.get(m);
      if (slot === undefined) return mm.empty >= 0 ? mm.empty : null;
      return slot >= 0 ? slot : null;
    },
  };
}
