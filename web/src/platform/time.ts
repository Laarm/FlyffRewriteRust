// Time sources (Plan/40 section 2.1, 14.2). `now()` is monotonic; the wire frame counter is derived from
// it. The sources are injectable (`setTimeSource`) so tests and the dev surfaces can drive a fake clock;
// the default is `performance.now()` / `Date.now()`.

/** Milliseconds per 60 Hz frame. */
export const FRAME_MS = 1000 / 60;

/** The two clocks the client reads (Plan/40 section 14.2 "clock"). */
export interface TimeSource {
  /** Monotonic milliseconds (`performance.now()`). */
  now(): number;
  /** Wall-clock milliseconds since the Unix epoch. */
  wall(): number;
}

const REAL_TIME: TimeSource = {
  now: () => performance.now(),
  wall: () => Date.now(),
};

let source: TimeSource = REAL_TIME;

/** Replaces the time sources (tests, /dev surfaces); `null` restores the real clocks. */
export function setTimeSource(next: TimeSource | null): void {
  source = next ?? REAL_TIME;
}

/** Monotonic milliseconds (`performance.now()` unless a fake source is installed). */
export function now(): number {
  return source.now();
}

/** Wall-clock milliseconds since the Unix epoch (display only, never used for simulation). */
export function wallNow(): number {
  return source.wall();
}

/**
 * The 60 Hz client frame counter used on the wire (`Hello.client_frame`, `ServerTimeReq`): whole
 * frames elapsed since the page started, as a `uint32`. The multiplication comes first so that exact
 * multiples of 1000/60 ms (e.g. 1000 ms) land on the frame boundary instead of one frame short.
 */
export function clientFrameAt(perfMs: number): number {
  return Math.floor((perfMs * 60) / 1000) >>> 0;
}

/** {@link clientFrameAt} of the current time. */
export function clientFrame(): number {
  return clientFrameAt(now());
}
