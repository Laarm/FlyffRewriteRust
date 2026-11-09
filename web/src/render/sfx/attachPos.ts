// World positions of the attachment points of a mover (Plan/41 section 12.4, 17.2; `CModelObject::GetHandPos` / `GetEventPos` followed
// by the mover's world matrix): the skill effects that hang on a hand or on the back of a character read them from the attachment
// record of the render view (`render/movers/attachments.ts`: RH, LH, SHIELD, KNUCKLE, WING as 3x4 matrices and 8 event points, all in
// the model space of the mover and in three.js space) and the mover's world matrix.
//
//   hand `h`    translation of the RH / LH matrix
//   event `i`   the point `events[i]`
//   world       `p' = M p` with the mover's 3x4 world matrix; the result is mapped back to LEFT-HANDED game space (`z` negated)

import { RENDER_VIEW } from '../../gen/sim/wire';
import { ATTACH_AT } from '../movers/attachments';

const MV = RENDER_VIEW.mover;
const STRIDE = RENDER_VIEW.MOVER_STRIDE;

/** What the lookups read of the view (the sim's `RenderViewReader`; any member missing makes them answer `false`). */
export interface AttachView {
  readonly valid?: boolean;
  indexOf?(objId: number): number;
  readonly movers?: Float32Array;
  readonly moversU32?: Uint32Array;
  readonly attach?: Float32Array;
}

/** The view as an {@link AttachView} when it has the readers, else `null`. */
export function asAttachView(view: unknown): AttachView | null {
  const v = view as AttachView | null;
  return v &&
    typeof v.indexOf === 'function' &&
    v.movers instanceof Float32Array &&
    v.moversU32 instanceof Uint32Array &&
    v.attach instanceof Float32Array &&
    v.valid !== false
    ? v
    : null;
}

type V3 = [number, number, number];

/** The record of `id`'s attachments: where it starts in `view.attach`, or -1. */
function attachBase(view: AttachView, id: number): { mover: number; attach: number } {
  const i = view.indexOf?.(id) ?? -1;
  const u = view.moversU32;
  if (i < 0 || !u) return { mover: -1, attach: -1 };
  const at = i * STRIDE;
  const flags = u[at + MV.flags] ?? 0;
  const index = u[at + MV.attachIndex] ?? RENDER_VIEW.NO_INDEX;
  if ((flags & RENDER_VIEW.moverFlags.HAS_ATTACH) === 0 || index === RENDER_VIEW.NO_INDEX) {
    return { mover: -1, attach: -1 };
  }
  return { mover: at, attach: index * RENDER_VIEW.ATTACH_STRIDE };
}

/** `M * (x, y, z)` with the mover's row-major 3x4 world matrix at `movers[at + MV.world ..]`, mapped to LH into `out`. */
function toWorldLh(
  movers: Float32Array,
  at: number,
  x: number,
  y: number,
  z: number,
  out: V3,
): void {
  const w = at + MV.world;
  const g = (k: number): number => movers[w + k] ?? 0;
  out[0] = g(0) * x + g(1) * y + g(2) * z + g(3);
  out[1] = g(4) * x + g(5) * y + g(6) * z + g(7);
  out[2] = -(g(8) * x + g(9) * y + g(10) * z + g(11));
}

/** The hand of mover `id` (0 = right, 1 = left) in the world (LH) into `out`; `false` without an attachment record. */
export function handWorldPos(view: AttachView, id: number, hand: 0 | 1, out: V3): boolean {
  const { mover, attach } = attachBase(view, id);
  const a = view.attach;
  const m = view.movers;
  if (mover < 0 || !a || !m) return false;
  const base = attach + (hand === 0 ? ATTACH_AT.rh : ATTACH_AT.lh);
  toWorldLh(m, mover, a[base + 3] ?? 0, a[base + 7] ?? 0, a[base + 11] ?? 0, out);
  return true;
}

/** Event point `index` (0..7) of mover `id` in the world (LH) into `out`; `false` without an attachment record. */
export function eventWorldPos(view: AttachView, id: number, index: number, out: V3): boolean {
  if (index < 0 || index > 7) return false;
  const { mover, attach } = attachBase(view, id);
  const a = view.attach;
  const m = view.movers;
  if (mover < 0 || !a || !m) return false;
  const base = attach + ATTACH_AT.events + index * 3;
  toWorldLh(m, mover, a[base] ?? 0, a[base + 1] ?? 0, a[base + 2] ?? 0, out);
  return true;
}

/** `CMover::GetRadius()` of mover `id` (the record's radius slot), `undefined` for an unknown mover. */
export function moverRadius(view: AttachView, id: number): number | undefined {
  const i = view.indexOf?.(id) ?? -1;
  const m = view.movers;
  if (i < 0 || !m) return undefined;
  return m[i * STRIDE + MV.radius];
}

/** `GetScale().x` of mover `id`: the length of the first axis of its world matrix (the model scale and the `SetScale` growth are in it); `undefined` for an unknown mover. */
export function moverScale(view: AttachView, id: number): number | undefined {
  const i = view.indexOf?.(id) ?? -1;
  const m = view.movers;
  if (i < 0 || !m) return undefined;
  const w = i * STRIDE + MV.world;
  return Math.hypot(m[w] ?? 0, m[w + 4] ?? 0, m[w + 8] ?? 0);
}
