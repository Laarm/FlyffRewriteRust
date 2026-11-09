// Interpolation of the map entities (Plan/25 section 3.7): positions are rendered ONE UPDATE INTERVAL IN THE PAST, linearly between the last
// two frames. An entity that changed in a frame slides from where the previous frame had it to where this one has it, over the interval
// measured between the two frames; moves longer than 30 m within one interval (teleports, respawns) snap; the yaw takes the shortest arc.
// Pure functions on plain numbers.

/** A move longer than this many metres within one interval is a teleport or a respawn: no slide. */
export const SNAP_M = 30;
/** The yaw is one byte: 256 steps for 360 degrees. */
export const YAW_STEPS = 256;

export const lerp = (a: number, b: number, f: number): number => a + (b - a) * f;

/** The yaw between `a` and `b` (0..255) at fraction `f`, taking the shortest way round. */
export function lerpYaw(a: number, b: number, f: number): number {
  let d = (b - a) % YAW_STEPS;
  if (d > YAW_STEPS / 2) d -= YAW_STEPS;
  else if (d < -YAW_STEPS / 2) d += YAW_STEPS;
  const r = (a + d * f) % YAW_STEPS;
  return r < 0 ? r + YAW_STEPS : r;
}

/** The slide fraction at `now` for a change that arrived at `t` and slides over `dur` ms (clamped to 0..1). */
export function slideFraction(now: number, t: number, dur: number): number {
  if (dur <= 0) return 1;
  const f = (now - t) / dur;
  return f <= 0 ? 0 : f >= 1 ? 1 : f;
}

/** `true` when the move from (x0, z0) to (x1, z1) is a teleport. */
export function isSnap(x0: number, z0: number, x1: number, z1: number): boolean {
  const dx = x1 - x0;
  const dz = z1 - z0;
  return dx * dx + dz * dz > SNAP_M * SNAP_M;
}

/**
 * The slide interval after a frame: half the previous estimate and half the gap since the last frame, kept within 0.5x to 4x the nominal
 * cadence of the tier (a stalled socket must not stretch the slide for ever, a burst must not make it instant).
 */
export function smoothInterval(prev: number, gapMs: number, nominalMs: number): number {
  const lo = nominalMs * 0.5;
  const hi = nominalMs * 4;
  const clamped = Math.min(hi, Math.max(lo, gapMs));
  if (!(prev > 0)) return clamped;
  return Math.min(hi, Math.max(lo, prev * 0.5 + clamped * 0.5));
}

/** The heading of a sprite in degrees clockwise from north (+z) for a server yaw byte: `(sin yaw, -cos yaw)` is the facing (D3D, 25 section 3.5). */
export function headingDegrees(yawByte: number): number {
  const yawDeg = (yawByte * 360) / YAW_STEPS;
  const h = (180 - yawDeg) % 360;
  return h < 0 ? h + 360 : h;
}
