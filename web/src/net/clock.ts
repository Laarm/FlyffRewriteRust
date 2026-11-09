// Server clock estimator (Plan/11 section 3.7/8.1, Plan/40 section 5.6): a pure estimator over
// `ServerTime`/`HelloAck` samples. No timers: the request cadence belongs to `net/connection.ts`.
//
// Frames are the 60 Hz counters of Plan/40 CA9. For one sample (the client sent a request at
// `clientFrameEcho`, received the reply at `clientFrameNow`; the server stamped `serverFrame` roughly
// halfway through):  offset = serverFrame - clientFrameEcho - rttFrames / 2.  The estimate is the median
// of the last five samples, so one delayed reply per five does not move it.

import { FRAME_MS } from '../platform/time';

/** Number of samples the medians are taken over. */
export const CLOCK_WINDOW = 5;

export interface ClockSample {
  /** `client_frame_echo` of the reply (the frame at which the request was sent). */
  readonly clientFrameEcho: number;
  /** The client frame when the reply was received. */
  readonly clientFrameNow: number;
  /** `n64(server_frame)` of the reply. */
  readonly serverFrame: number;
  /** `n64(server_unix_ms)` of the reply. */
  readonly serverUnixMs: number;
  /** `performance.now()` when the reply was received. */
  readonly perfNowMs: number;
  /** `client_perf_ms_echo` of `ServerTime` (the exact send time); gives a millisecond-precise RTT. */
  readonly clientPerfEchoMs?: number;
}

function median(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  const hi = sorted[mid] ?? 0;
  if (sorted.length % 2 === 1) return hi;
  const lo = sorted[mid - 1] ?? hi;
  return (lo + hi) / 2;
}

export class ClockEstimator {
  private readonly offsets: number[] = [];
  private readonly unixOffsets: number[] = [];
  private readonly rtts: number[] = [];
  private count = 0;

  /** Adds one sample; all frame values are already converted with `n64()`. */
  addSample(s: ClockSample): void {
    const rttMs =
      s.clientPerfEchoMs === undefined
        ? ((s.clientFrameNow - s.clientFrameEcho) >>> 0) * FRAME_MS
        : Math.max(0, s.perfNowMs - s.clientPerfEchoMs);
    const rttFrames = rttMs / FRAME_MS;
    this.push(this.offsets, s.serverFrame - s.clientFrameEcho - rttFrames / 2);
    // the server stamped the reply about half an RTT before we received it
    this.push(this.unixOffsets, s.serverUnixMs + rttMs / 2 - s.perfNowMs);
    this.push(this.rtts, rttMs);
    this.count += 1;
  }

  private push(window: number[], value: number): void {
    window.push(value);
    if (window.length > CLOCK_WINDOW) window.shift();
  }

  /** Median offset `server frame - client frame` of the last samples, in 60 Hz frames (0 before any sample). */
  get offset(): number {
    return this.offsets.length === 0 ? 0 : median(this.offsets);
  }

  /** Median round-trip time in ms (0 before any sample). */
  get rttMs(): number {
    return this.rtts.length === 0 ? 0 : median(this.rtts);
  }

  /** `true` once at least one sample arrived. */
  get synced(): boolean {
    return this.count > 0;
  }

  /** Total samples received. */
  get samples(): number {
    return this.count;
  }

  /** The estimated server frame at the client frame `clientFrame`. */
  serverFrameNow(clientFrame: number): number {
    return clientFrame + this.offset;
  }

  /** The estimated server wall clock (unix ms) at `perfNowMs`, for the `*_at` fields (Plan/11 section 5.8). */
  serverUnixMsNow(perfNowMs: number): number {
    return perfNowMs + (this.unixOffsets.length === 0 ? 0 : median(this.unixOffsets));
  }

  /** Forgets every sample (a new session). */
  reset(): void {
    this.offsets.length = 0;
    this.unixOffsets.length = 0;
    this.rtts.length = 0;
    this.count = 0;
  }
}
