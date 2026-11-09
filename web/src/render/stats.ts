// Per-frame counters of the renderer (Plan/41 section 24), exposed at `window.__flyffDebug.render`. The collector
// is plain data: the renderer writes into it, the probe section and the dev pages read snapshots.

import type { QualityTier } from './quality';

/** Names of the passes of Plan/41 section 2.4 (draw calls are counted per pass). */
export type PassName =
  | 'clear'
  | 'sky'
  | 'terrain'
  | 'opaque'
  | 'blended'
  | 'highlight'
  | 'partsFx'
  | 'particles'
  | 'blobShadows'
  | 'water'
  | 'ribbons'
  | 'underwater'
  | 'sfx'
  | 'translucent'
  | 'weather'
  | 'flare'
  | 'ramp'
  | 'overlay';

/** `RenderStats` of Plan/41 section 24 (+ the frame-time percentiles the perf scenes report). */
export interface RenderStats {
  /** `performance.now()` around `draw` (scene update + submission, not GPU time). */
  cpuDrawMs: number;
  cpuDrawMsP50: number;
  cpuDrawMsP95: number;
  /** `EXT_disjoint_timer_query_webgl2`, `null` when unavailable. */
  gpuMs: number | null;
  /** Draw calls of the last frame per pass (passes without calls are omitted). */
  drawCalls: Partial<Record<PassName, number>>;
  /** Sum of {@link drawCalls}. */
  drawCallsTotal: number;
  triangles: number;
  /** Estimated GPU memory of resident textures and geometry in MiB. */
  vramMb: number;
  /** Bytes uploaded by the upload queue in the last frame. */
  uploadBytes: number;
  visibleMovers: number;
  visibleStatics: number;
  landsResident: number;
  sfxInstances: number;
  sfxQuads: number;
  sfxDropped: number;
  paletteMatrices: number;
  paletteBytes: number;
  missingTextures: number;
  shaderPermutations: number;
  /** `renderer.info.programs.length`. */
  programs: number;
  /** `renderer.info.memory.textures`. */
  textures: number;
  /** Frames drawn since the renderer was created. */
  frames: number;
  /** Times the WebGL context was lost / restored. */
  contextLost: number;
  contextRestored: number;
  tier: QualityTier;
}

/**
 * `__flyffDebug.render()` (Plan/60 section 13.2, WP-REND-17): the five counters of the last drawn frame the perf and visual-QA
 * specs read, exactly these keys.
 */
export interface ProbeRenderCounters {
  /** {@link RenderStats.drawCallsTotal}: the draw calls of all passes of the last frame. */
  readonly drawCalls: number;
  readonly triangles: number;
  readonly programs: number;
  readonly textures: number;
  /** {@link RenderStats.uploadBytes}: bytes the upload queue sent to the GPU in the last frame. */
  readonly uploadsBytesLastFrame: number;
}

/** The five counters of `render()` over a stats snapshot. */
export function renderCounters(stats: RenderStats): ProbeRenderCounters {
  return {
    drawCalls: stats.drawCallsTotal,
    triangles: stats.triangles,
    programs: stats.programs,
    textures: stats.textures,
    uploadsBytesLastFrame: stats.uploadBytes,
  };
}

export function emptyStats(tier: QualityTier = 'medium'): RenderStats {
  return {
    cpuDrawMs: 0,
    cpuDrawMsP50: 0,
    cpuDrawMsP95: 0,
    gpuMs: null,
    drawCalls: {},
    drawCallsTotal: 0,
    triangles: 0,
    vramMb: 0,
    uploadBytes: 0,
    visibleMovers: 0,
    visibleStatics: 0,
    landsResident: 0,
    sfxInstances: 0,
    sfxQuads: 0,
    sfxDropped: 0,
    paletteMatrices: 0,
    paletteBytes: 0,
    missingTextures: 0,
    shaderPermutations: 0,
    programs: 0,
    textures: 0,
    frames: 0,
    contextLost: 0,
    contextRestored: 0,
    tier,
  };
}

/** The p-th percentile (0..1) of `values` (nearest rank, `0` for an empty list). */
export function percentile(values: readonly number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.min(sorted.length - 1, Math.max(0, Math.ceil(p * sorted.length) - 1));
  return sorted[rank] ?? 0;
}

/** Mutable collector behind {@link RenderStats}. */
export class StatsCollector {
  readonly stats: RenderStats;
  private readonly cpuRing: number[] = [];
  private readonly ringSize: number;

  constructor(tier: QualityTier = 'medium', ringSize = 240) {
    this.stats = emptyStats(tier);
    this.ringSize = ringSize;
  }

  /** Records the CPU time of one `draw` and updates the percentiles. */
  recordCpuMs(ms: number): void {
    this.stats.cpuDrawMs = ms;
    this.cpuRing.push(ms);
    if (this.cpuRing.length > this.ringSize) this.cpuRing.shift();
    this.stats.cpuDrawMsP50 = percentile(this.cpuRing, 0.5);
    this.stats.cpuDrawMsP95 = percentile(this.cpuRing, 0.95);
  }

  /** Replaces the per-pass draw call counts of the last frame. */
  setDrawCalls(perPass: Partial<Record<PassName, number>>, triangles: number): void {
    this.stats.drawCalls = { ...perPass };
    let total = 0;
    for (const n of Object.values(perPass)) total += n;
    this.stats.drawCallsTotal = total;
    this.stats.triangles = triangles;
  }

  /** A copy that the caller may keep. */
  snapshot(): RenderStats {
    return { ...this.stats, drawCalls: { ...this.stats.drawCalls } };
  }

  /** The recorded CPU times (for the perf report), oldest first. */
  cpuSamples(): readonly number[] {
    return this.cpuRing;
  }
}
