// Capability probes (Plan/40 section 3.2, WP-CLIENT-11). No secure-context API is probed or required (D3: plain HTTP
// on a LAN is the primary deployment). `detectCapabilities()` is side-effect free (the three flags the context needs);
// `probeCapabilities()` adds the WebGL probe (a throw-away canvas), the S3TC extension, hardware numbers and the quality
// tier RECOMMENDATION (41 R13): advisory only, logged once, never applied automatically (owner 2026-10-29). The renderer
// starts in the tier of `startTier()`: Medium (the original options) unless the player asks for another one in the address.

export type QualityTier = 'low' | 'medium' | 'high';

export interface Capabilities {
  readonly webAssembly: boolean;
  readonly worker: boolean;
  readonly webgl2: boolean;
  /** `WEBGL_compressed_texture_s3tc`: `false` makes the worker pool decode `dds` to RGBA8 (`dds_bc`). */
  readonly s3tc?: boolean;
  /** `WebAssembly.compileStreaming` exists (else `compile(arrayBuffer)`). */
  readonly compileStreaming?: boolean;
  readonly hardwareConcurrency?: number;
  /** GiB, where `navigator.deviceMemory` exists (Chromium). */
  readonly deviceMemory?: number;
  readonly maxTextureSize?: number;
  readonly maxVertexUniformVectors?: number;
  /** `WEBGL_debug_renderer_info` UNMASKED_RENDERER_WEBGL (when exposed). */
  readonly renderer?: string;
  /** SwiftShader, llvmpipe, Microsoft Basic Render: no GPU. */
  readonly softwareGl?: boolean;
  /** Fetch concurrency: 6 on http (HTTP/1.1), 12 on https (h2). */
  readonly fetchConcurrency?: number;
  /** `document.fonts.check('12px Verdana')` (PL-126: Verdana when present, else DejaVu Sans). */
  readonly verdana?: boolean;
  /**
   * The tier the device could manage (software GL or a small machine: `low`, a big one: `high`). Advisory only: logged once,
   * never applied automatically (owner 2026-10-29); the renderer starts in {@link startTier}.
   */
  readonly recommendedTier?: QualityTier;
}

/** Feature detection without side effects. */
export function detectCapabilities(): Capabilities {
  return {
    webAssembly: typeof WebAssembly === 'object',
    worker: typeof Worker === 'function',
    webgl2: typeof WebGL2RenderingContext === 'function',
  };
}

/** The numbers of `navigator` the probe reads (`deviceMemory` is Chromium-only and not in lib.dom). */
export interface NavLike {
  readonly hardwareConcurrency?: number;
  readonly deviceMemory?: number;
}

/** The environment of the probes (tests pass fakes). */
export interface ProbeEnv {
  readonly createCanvas?: () => { getContext(id: string): unknown } | null;
  readonly navigator?: NavLike;
  readonly protocol?: string;
  readonly compileStreaming?: boolean;
  readonly fontsCheck?: (font: string) => boolean;
}

interface GlLike {
  getExtension(name: string): unknown;
  getParameter(p: number): unknown;
  readonly MAX_TEXTURE_SIZE?: number;
  readonly MAX_VERTEX_UNIFORM_VECTORS?: number;
}

const SOFTWARE = /swiftshader|llvmpipe|softpipe|software|basic render|microsoft basic/i;

/**
 * The quality tier a device could manage (41 R13): a software renderer or a small machine is `low`. A recommendation for the
 * log and the options window; the renderer never starts in it by itself (owner 2026-10-29).
 */
export function proposeQualityTier(
  c: Pick<Capabilities, 'softwareGl' | 'deviceMemory' | 'hardwareConcurrency' | 'maxTextureSize'>,
): QualityTier {
  if (c.softwareGl === true) return 'low';
  const mem = c.deviceMemory;
  const cores = c.hardwareConcurrency ?? 4;
  if ((mem !== undefined && mem <= 2) || cores <= 2) return 'low';
  if ((mem === undefined || mem >= 8) && cores >= 6 && (c.maxTextureSize ?? 0) >= 16384)
    return 'high';
  return 'medium';
}

/**
 * The tier the renderer starts in: the `tier` parameter of the page address when it is exactly `low`, `medium` or `high`, else
 * `medium` (the original options). Never derived from the device (owner 2026-10-29); `search` is `location.search`.
 */
export function startTier(search: string): QualityTier {
  const t = new URLSearchParams(search).get('tier');
  return t === 'low' || t === 'medium' || t === 'high' ? t : 'medium';
}

function navigatorOf(env: ProbeEnv): NavLike | undefined {
  if (env.navigator) return env.navigator;
  return typeof navigator === 'object' ? navigator : undefined;
}

/** A WebGL2 context on a throw-away canvas, `null` when there is none. */
function openWebgl2(env: ProbeEnv): GlLike | null {
  try {
    const canvas =
      env.createCanvas?.() ??
      (typeof document === 'object' ? document.createElement('canvas') : null);
    return (canvas?.getContext('webgl2') as GlLike | null | undefined) ?? null;
  } catch {
    return null;
  }
}

/**
 * The full probe. A page without WebGL2 or WebAssembly cannot run the client (`FatalScreen("unsupported browser")`):
 * the caller checks `webgl2` and `webAssembly`.
 */
export function probeCapabilities(env: ProbeEnv = {}): Capabilities {
  const base = detectCapabilities();
  const nav = navigatorOf(env);
  const gl = openWebgl2(env);
  let s3tc = false;
  let renderer: string | undefined;
  let maxTextureSize: number | undefined;
  let maxVertexUniformVectors: number | undefined;
  if (gl) {
    s3tc = gl.getExtension('WEBGL_compressed_texture_s3tc') !== null;
    const info = gl.getExtension('WEBGL_debug_renderer_info') as {
      UNMASKED_RENDERER_WEBGL?: number;
    } | null;
    if (info?.UNMASKED_RENDERER_WEBGL !== undefined) {
      const r = gl.getParameter(info.UNMASKED_RENDERER_WEBGL);
      if (typeof r === 'string') renderer = r;
    }
    const mt = gl.getParameter(gl.MAX_TEXTURE_SIZE ?? 0x0d33);
    if (typeof mt === 'number') maxTextureSize = mt;
    const mv = gl.getParameter(gl.MAX_VERTEX_UNIFORM_VECTORS ?? 0x8dfb);
    if (typeof mv === 'number') maxVertexUniformVectors = mv;
  }
  const protocol = env.protocol ?? (typeof location === 'object' ? location.protocol : 'http:');
  // the probe context is not needed any more: give its slot back (browsers cap the live contexts per page)
  (
    gl?.getExtension('WEBGL_lose_context') as { loseContext(): void } | null | undefined
  )?.loseContext();
  const caps: Capabilities = {
    ...base,
    webgl2: base.webgl2 && gl !== null,
    s3tc,
    compileStreaming:
      env.compileStreaming ??
      (typeof WebAssembly === 'object' && typeof WebAssembly.compileStreaming === 'function'),
    ...(nav?.hardwareConcurrency === undefined
      ? {}
      : { hardwareConcurrency: nav.hardwareConcurrency }),
    ...(nav?.deviceMemory === undefined ? {} : { deviceMemory: nav.deviceMemory }),
    ...(maxTextureSize === undefined ? {} : { maxTextureSize }),
    ...(maxVertexUniformVectors === undefined ? {} : { maxVertexUniformVectors }),
    ...(renderer === undefined ? {} : { renderer }),
    softwareGl: renderer !== undefined && SOFTWARE.test(renderer),
    fetchConcurrency: protocol === 'https:' ? 12 : 6,
    ...(env.fontsCheck
      ? { verdana: env.fontsCheck('12px Verdana') }
      : typeof document === 'object' && 'fonts' in document
        ? { verdana: document.fonts.check('12px Verdana') }
        : {}),
  };
  return { ...caps, recommendedTier: proposeQualityTier(caps) };
}

/** Why the page cannot run the client, or `null` (Plan/40 section 3.1 B0). */
export function unsupportedReason(c: Capabilities): string | null {
  if (!c.webAssembly) return 'This browser does not support WebAssembly.';
  if (!c.webgl2) return 'This browser does not support WebGL 2.';
  if (!c.worker) return 'This browser does not support Web Workers.';
  return null;
}

/** Worker pool size: `clamp(hardwareConcurrency - 2, 2, 4)` (Plan/01 section 6.8). */
export function workerCount(c: Pick<Capabilities, 'hardwareConcurrency'>): number {
  return Math.min(4, Math.max(2, (c.hardwareConcurrency ?? 4) - 2));
}
