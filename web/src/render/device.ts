// The WebGL2 device of the renderer (Plan/41 section 4, WP-REND-01): the `WebGLRenderer` with the GAMMA-NAIVE colour
// pipeline (R1), the DPR cap, the extension probe, the off-screen target and the final gamma/bright/contrast ramp
// pass (section 4.3), context loss and restore (section 4.2).
//
// Colour pipeline: `ColorManagement.enabled = false`, `outputColorSpace = LinearSRGBColorSpace`, no tone mapping,
// every texture `NoColorSpace`: the numbers are the original 8-bit values and blending happens on them.
//
// Frame: `beginFrame(clear)` binds the off-screen RGBA8 target and clears colour+depth with the fog colour,
// `renderLayer(scene, camera, layer)` renders one pass group into it, `present()` draws the full-screen ramp pass
// (a 256-entry per-channel LUT, `R8` texture) into the canvas. Overlays that must be ramped as well draw after
// `present` through the same LUT (section 4.3).

import {
  BufferAttribute,
  BufferGeometry,
  ColorManagement,
  DataTexture,
  GLSL3,
  LinearSRGBColorSpace,
  Mesh,
  NearestFilter,
  NoColorSpace,
  NoToneMapping,
  OrthographicCamera,
  RawShaderMaterial,
  RedFormat,
  RGBAFormat,
  Scene,
  UnsignedByteType,
  WebGLRenderer,
  WebGLRenderTarget,
  ClampToEdgeWrapping,
  type Camera,
} from 'three';
import { probeCaps, type DeviceCaps, type ProbeOptions } from './gl/caps';
import { DEFAULT_RENDER_OPTIONS, type RenderOptions } from './quality';
import { buildRampLut } from './rampLut';
import type { RGB } from './types';

/** WebGL2 is missing or the context could not be created: a fatal client error (doc 40). */
export class RenderUnsupportedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RenderUnsupportedError';
  }
}

/** The renderer settings of Plan/41 section 4.1 that the device owns (testable without a GL context). */
export interface ColorPipelineTarget {
  outputColorSpace: string;
  toneMapping: number;
}

/**
 * Applies the gamma-naive colour pipeline (R1): three.js colour management off, linear output, no tone mapping.
 * `ColorManagement` is a process-wide singleton; it is switched off here and never on again.
 */
export function configureColorPipeline(renderer: ColorPipelineTarget): void {
  ColorManagement.enabled = false;
  renderer.outputColorSpace = LinearSRGBColorSpace;
  renderer.toneMapping = NoToneMapping;
}

/** `true` when the pipeline settings of `renderer` are the gamma-naive ones. */
export function isGammaNaive(renderer: ColorPipelineTarget): boolean {
  return (
    !ColorManagement.enabled &&
    renderer.outputColorSpace === LinearSRGBColorSpace &&
    renderer.toneMapping === NoToneMapping
  );
}

export interface DeviceOptions extends ProbeOptions {
  readonly options?: RenderOptions;
  /** Keep the drawing buffer readable after the frame (dev pages, screenshots). */
  readonly preserveDrawingBuffer?: boolean;
  /** `devicePixelRatio` provider (tests). */
  readonly devicePixelRatio?: () => number;
}

const RAMP_VERTEX = /* glsl */ `
precision highp float;
in vec3 position;
out vec2 vUv;
void main() {
  vUv = position.xy * 0.5 + 0.5;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}
`;

const RAMP_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uScene;
uniform sampler2D uLut;
in vec2 vUv;
layout(location = 0) out highp vec4 fragColor;
void main() {
  vec3 c = texture(uScene, vUv).rgb;
  vec3 i = floor(c * 255.0 + 0.5);
  fragColor = vec4(
    texelFetch(uLut, ivec2(int(i.r), 0), 0).r,
    texelFetch(uLut, ivec2(int(i.g), 0), 0).r,
    texelFetch(uLut, ivec2(int(i.b), 0), 0).r,
    1.0
  );
}
`;

/** GPU time of a frame through `EXT_disjoint_timer_query_webgl2` (asynchronous: the result of an earlier frame). */
export class GpuTimer {
  private readonly pending: WebGLQuery[] = [];
  private active: WebGLQuery | null = null;
  /** The newest finished measurement in ms, `null` until one is available. */
  last: number | null = null;

  constructor(
    private readonly gl: WebGL2RenderingContext,
    private readonly ext: { readonly TIME_ELAPSED_EXT: number; readonly GPU_DISJOINT_EXT: number },
  ) {}

  begin(): void {
    if (this.active || this.pending.length > 4) return;
    const q = this.gl.createQuery();
    this.gl.beginQuery(this.ext.TIME_ELAPSED_EXT, q);
    this.active = q;
  }

  end(): void {
    if (!this.active) return;
    this.gl.endQuery(this.ext.TIME_ELAPSED_EXT);
    this.pending.push(this.active);
    this.active = null;
  }

  /** Collects finished queries; call once per frame. */
  poll(): void {
    const disjoint = Boolean(this.gl.getParameter(this.ext.GPU_DISJOINT_EXT));
    while (this.pending.length > 0) {
      const q = this.pending[0];
      if (!q) break;
      if (!this.gl.getQueryParameter(q, this.gl.QUERY_RESULT_AVAILABLE)) break;
      this.pending.shift();
      if (!disjoint) this.last = Number(this.gl.getQueryParameter(q, this.gl.QUERY_RESULT)) / 1e6;
      this.gl.deleteQuery(q);
    }
  }
}

/** The WebGL2 device: renderer, caps, off-screen target and ramp pass. */
export class RenderDevice {
  readonly renderer: WebGLRenderer;
  readonly gl: WebGL2RenderingContext;
  /**
   * The device capabilities. The object keeps its identity for the lifetime of the device (the texture cache holds
   * it); a context restore re-enables the extensions and updates its fields in place.
   */
  readonly caps: DeviceCaps;
  readonly canvas: HTMLCanvasElement;
  options: RenderOptions;
  /** Times the context was lost and restored (stats). */
  contextLost = 0;
  contextRestored = 0;
  isLost = false;
  /** `EXT_disjoint_timer_query_webgl2` timer, `null` without the extension (re-created after a context restore). */
  gpuTimer: GpuTimer | null;
  /** The `TextureCache` view of this device (the WebGL2 context and three.js's texture-binding state). */
  readonly textureHost: {
    getContext(): WebGL2RenderingContext;
    readonly state: WebGLRenderer['state'];
  };

  private readonly probeOptions: ProbeOptions;
  private target: WebGLRenderTarget | null = null;
  private readonly rampScene = new Scene();
  private readonly rampCamera = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly rampMaterial: RawShaderMaterial;
  private readonly lutTexture: DataTexture;
  private lut: Uint8Array;
  private cssW = 1;
  private cssH = 1;
  private readonly dpr: () => number;
  private readonly lostListeners = new Set<() => void>();
  private readonly restoredListeners = new Set<() => void>();
  private readonly onLost: (e: Event) => void;
  private readonly onRestored: () => void;

  constructor(canvas: HTMLCanvasElement, opts: DeviceOptions = {}) {
    this.canvas = canvas;
    this.options = opts.options ?? { ...DEFAULT_RENDER_OPTIONS };
    this.dpr =
      opts.devicePixelRatio ??
      (() => (typeof window === 'undefined' ? 1 : window.devicePixelRatio));
    let renderer: WebGLRenderer;
    try {
      renderer = new WebGLRenderer({
        canvas,
        antialias: false,
        alpha: false,
        depth: true,
        stencil: false,
        premultipliedAlpha: true,
        preserveDrawingBuffer: opts.preserveDrawingBuffer === true,
        powerPreference: 'high-performance',
      });
    } catch (e) {
      throw new RenderUnsupportedError(
        `WebGL2 is not available: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
    this.renderer = renderer;
    configureColorPipeline(renderer);
    renderer.autoClear = false;
    renderer.info.autoReset = false;
    this.gl = renderer.getContext() as WebGL2RenderingContext;
    this.textureHost = { getContext: () => this.gl, state: renderer.state };
    this.probeOptions = opts;
    this.caps = probeCaps(this.gl, opts);
    this.gpuTimer = this.makeTimer();

    this.lut = buildRampLut(this.options.gamma, this.options.bright, this.options.contrast);
    this.lutTexture = new DataTexture(this.lut, 256, 1, RedFormat, UnsignedByteType);
    this.lutTexture.minFilter = NearestFilter;
    this.lutTexture.magFilter = NearestFilter;
    this.lutTexture.wrapS = ClampToEdgeWrapping;
    this.lutTexture.wrapT = ClampToEdgeWrapping;
    this.lutTexture.unpackAlignment = 1;
    this.lutTexture.colorSpace = NoColorSpace;
    this.lutTexture.generateMipmaps = false;
    this.lutTexture.needsUpdate = true;

    const tri = new BufferGeometry();
    tri.setAttribute(
      'position',
      new BufferAttribute(new Float32Array([-1, -1, 0, 3, -1, 0, -1, 3, 0]), 3),
    );
    this.rampMaterial = new RawShaderMaterial({
      glslVersion: GLSL3,
      vertexShader: RAMP_VERTEX,
      fragmentShader: RAMP_FRAGMENT,
      uniforms: { uScene: { value: null }, uLut: { value: this.lutTexture } },
      depthTest: false,
      depthWrite: false,
    });
    const quad = new Mesh(tri, this.rampMaterial);
    quad.frustumCulled = false;
    this.rampScene.add(quad);

    this.onLost = (e: Event) => {
      e.preventDefault();
      this.isLost = true;
      this.contextLost++;
      for (const l of [...this.lostListeners]) l();
    };
    this.onRestored = () => {
      // three.js has re-initialised its state in its own listener (registered first)
      this.isLost = false;
      this.contextRestored++;
      this.target = null;
      // the new context starts without the extensions: enable them again and update `caps` in place
      Object.assign(this.caps, probeCaps(this.gl, this.probeOptions));
      this.gpuTimer = this.makeTimer();
      for (const l of [...this.restoredListeners]) l();
    };
    canvas.addEventListener('webglcontextlost', this.onLost);
    canvas.addEventListener('webglcontextrestored', this.onRestored);
  }

  private makeTimer(): GpuTimer | null {
    if (!this.caps.timerQuery) return null;
    const ext = this.gl.getExtension('EXT_disjoint_timer_query_webgl2') as {
      TIME_ELAPSED_EXT: number;
      GPU_DISJOINT_EXT: number;
    } | null;
    return ext ? new GpuTimer(this.gl, ext) : null;
  }

  // ---- size ---------------------------------------------------------------------------------------------

  /** The effective pixel ratio: `min(devicePixelRatio, dprCap)`. */
  get pixelRatio(): number {
    return Math.max(0.25, Math.min(this.dpr(), this.options.dprCap));
  }

  get cssWidth(): number {
    return this.cssW;
  }

  get cssHeight(): number {
    return this.cssH;
  }

  get aspect(): number {
    return this.cssW / this.cssH;
  }

  /** Drawing buffer size in pixels (what three.js sets on the canvas: `floor(css * pixelRatio)`). */
  get bufferWidth(): number {
    return Math.max(1, Math.floor(this.cssW * this.pixelRatio));
  }

  get bufferHeight(): number {
    return Math.max(1, Math.floor(this.cssH * this.pixelRatio));
  }

  /** Sets the CSS size of the canvas and resizes the drawing buffer and the off-screen target. */
  resize(cssWidth: number, cssHeight: number): void {
    this.cssW = Math.max(1, cssWidth);
    this.cssH = Math.max(1, cssHeight);
    this.renderer.setPixelRatio(this.pixelRatio);
    this.renderer.setSize(this.cssW, this.cssH, false);
    if (this.target) {
      this.target.setSize(this.bufferWidth, this.bufferHeight);
    }
  }

  /** Applies new options (ramp, DPR cap, MSAA). */
  setOptions(options: RenderOptions): void {
    const msaaChanged = options.msaa !== this.options.msaa;
    this.options = options;
    this.setRamp(options.gamma, options.bright, options.contrast);
    this.resize(this.cssW, this.cssH);
    if (msaaChanged && this.target) {
      this.target.dispose();
      this.target = null;
    }
  }

  // ---- ramp ---------------------------------------------------------------------------------------------

  /** Rebuilds the LUT texture (an option change). */
  setRamp(gamma: number, bright: number, contrast: number): void {
    this.lut = buildRampLut(gamma, bright, contrast);
    const data = this.lutTexture.image.data as Uint8Array | null;
    if (data) data.set(this.lut);
    this.lutTexture.needsUpdate = true;
  }

  /** The current ramp table (a copy). */
  rampTable(): Uint8Array {
    return this.lut.slice();
  }

  /** The R8 LUT texture (the overlay pass samples it too). */
  get rampTexture(): DataTexture {
    return this.lutTexture;
  }

  // ---- frame --------------------------------------------------------------------------------------------

  private ensureTarget(): WebGLRenderTarget {
    if (!this.target) {
      this.target = new WebGLRenderTarget(this.bufferWidth, this.bufferHeight, {
        format: RGBAFormat,
        type: UnsignedByteType,
        minFilter: NearestFilter,
        magFilter: NearestFilter,
        depthBuffer: true,
        stencilBuffer: false,
        generateMipmaps: false,
        samples: this.options.msaa,
      });
      this.target.texture.colorSpace = NoColorSpace;
    }
    return this.target;
  }

  /** Starts a frame: resets the info counters, binds the off-screen target and clears colour+depth. */
  beginFrame(clear: RGB): void {
    this.renderer.info.reset();
    this.gpuTimer?.begin();
    const t = this.ensureTarget();
    this.renderer.setRenderTarget(t);
    this.renderer.setClearColor(clearColorOf(clear), 1);
    this.renderer.clear(true, true, false);
  }

  /** Renders one pass group (`layer`) of `scene` into the off-screen target. */
  renderLayer(scene: Scene, camera: Camera, layer: number | null, sortObjects = true): void {
    if (layer !== null) camera.layers.set(layer);
    else camera.layers.enableAll();
    this.renderer.sortObjects = sortObjects;
    this.renderer.setRenderTarget(this.ensureTarget());
    this.renderer.render(scene, camera);
  }

  /** Draw calls and triangles accumulated since `beginFrame`. */
  get callsSoFar(): number {
    return this.renderer.info.render.calls;
  }

  get trianglesSoFar(): number {
    return this.renderer.info.render.triangles;
  }

  /** The ramp pass: the off-screen target through the LUT into the canvas. */
  present(): void {
    const t = this.ensureTarget();
    const scene = this.rampMaterial.uniforms['uScene'];
    if (scene) scene.value = t.texture;
    this.renderer.setRenderTarget(null);
    this.renderer.render(this.rampScene, this.rampCamera);
    this.gpuTimer?.end();
    this.gpuTimer?.poll();
  }

  // ---- context loss -------------------------------------------------------------------------------------

  onContextLost(listener: () => void): () => void {
    this.lostListeners.add(listener);
    return () => this.lostListeners.delete(listener);
  }

  onContextRestored(listener: () => void): () => void {
    this.restoredListeners.add(listener);
    return () => this.restoredListeners.delete(listener);
  }

  /** Test helper: `WEBGL_lose_context.loseContext()`; restores after `restoreAfterMs` when given. */
  loseContextForTest(restoreAfterMs?: number): boolean {
    const ext = this.gl.getExtension('WEBGL_lose_context');
    if (!ext) return false;
    ext.loseContext();
    if (restoreAfterMs !== undefined)
      setTimeout(() => {
        ext.restoreContext();
      }, restoreAfterMs);
    return true;
  }

  /** Reads pixels of the default framebuffer (RGBA, bottom-up rows) right after a frame (same task). */
  readPixels(x: number, y: number, w: number, h: number): Uint8Array {
    const out = new Uint8Array(w * h * 4);
    this.renderer.setRenderTarget(null);
    this.gl.readPixels(x, y, w, h, this.gl.RGBA, this.gl.UNSIGNED_BYTE, out);
    return out;
  }

  /** Reads pixels of the off-screen target (the scene before the ramp). */
  readTargetPixels(x: number, y: number, w: number, h: number): Uint8Array {
    const out = new Uint8Array(w * h * 4);
    this.renderer.readRenderTargetPixels(this.ensureTarget(), x, y, w, h, out);
    return out;
  }

  dispose(): void {
    this.canvas.removeEventListener('webglcontextlost', this.onLost);
    this.canvas.removeEventListener('webglcontextrestored', this.onRestored);
    this.target?.dispose();
    this.lutTexture.dispose();
    this.rampMaterial.dispose();
    this.renderer.dispose();
  }
}

/** The `Color`-like argument of `setClearColor` without importing `Color` (numbers 0..1, gamma-naive). */
function clearColorOf(rgb: RGB): number {
  const to8 = (v: number): number => Math.min(255, Math.max(0, Math.round(v * 255)));
  return (to8(rgb[0]) << 16) | (to8(rgb[1]) << 8) | to8(rgb[2]);
}
