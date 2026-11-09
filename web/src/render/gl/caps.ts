// Capabilities of the WebGL2 device (Plan/41 section 4.1, "extensions probed"). `probeCaps` enables the
// extensions the renderer uses: `WEBGL_compressed_texture_s3tc` (required for the BC passthrough, else the BC
// fallback of section 6.1 runs), `EXT_texture_filter_anisotropic` (High tier only), `EXT_color_buffer_float` and
// `OES_texture_float_linear` (not required: the palette uses NEAREST), `EXT_disjoint_timer_query_webgl2`.

export interface DeviceCaps {
  /** `WEBGL_compressed_texture_s3tc` is enabled (and not disabled by the test flag). */
  readonly s3tc: boolean;
  /** The anisotropy parameter enum, `null` without `EXT_texture_filter_anisotropic`. */
  readonly anisotropyParam: number | null;
  readonly maxAnisotropy: number;
  readonly floatLinear: boolean;
  readonly colorBufferFloat: boolean;
  /** `EXT_disjoint_timer_query_webgl2`. */
  readonly timerQuery: boolean;
  readonly maxTextureSize: number;
  readonly maxTextureUnits: number;
  /** `WEBGL_debug_renderer_info` strings ("" when unavailable). */
  readonly rendererString: string;
  readonly vendorString: string;
  /** SwiftShader, llvmpipe and friends: timings are not representative. */
  readonly software: boolean;
}

export interface ProbeOptions {
  /** Test flag: behave as if `WEBGL_compressed_texture_s3tc` did not exist (forces the BC fallback). */
  readonly forceNoS3tc?: boolean;
}

const SOFTWARE_RE = /swiftshader|llvmpipe|softpipe|software|basic render/i;

/** Enables and reads the extensions. */
export function probeCaps(gl: WebGL2RenderingContext, options: ProbeOptions = {}): DeviceCaps {
  const s3tcExt =
    options.forceNoS3tc === true ? null : gl.getExtension('WEBGL_compressed_texture_s3tc');
  const aniso = gl.getExtension('EXT_texture_filter_anisotropic');
  const debug = gl.getExtension('WEBGL_debug_renderer_info');
  const rendererString = debug ? String(gl.getParameter(debug.UNMASKED_RENDERER_WEBGL)) : '';
  const vendorString = debug ? String(gl.getParameter(debug.UNMASKED_VENDOR_WEBGL)) : '';
  return {
    s3tc: s3tcExt !== null,
    anisotropyParam: aniso ? aniso.TEXTURE_MAX_ANISOTROPY_EXT : null,
    maxAnisotropy: aniso ? Number(gl.getParameter(aniso.MAX_TEXTURE_MAX_ANISOTROPY_EXT)) : 1,
    floatLinear: gl.getExtension('OES_texture_float_linear') !== null,
    colorBufferFloat: gl.getExtension('EXT_color_buffer_float') !== null,
    timerQuery: gl.getExtension('EXT_disjoint_timer_query_webgl2') !== null,
    maxTextureSize: Number(gl.getParameter(gl.MAX_TEXTURE_SIZE)),
    maxTextureUnits: Number(gl.getParameter(gl.MAX_TEXTURE_IMAGE_UNITS)),
    rendererString,
    vendorString,
    software: SOFTWARE_RE.test(rendererString),
  };
}

/** Capabilities of a fake device (tests): everything the texture planning needs. */
export function fakeCaps(overrides: Partial<DeviceCaps> = {}): DeviceCaps {
  return {
    s3tc: true,
    anisotropyParam: null,
    maxAnisotropy: 1,
    floatLinear: false,
    colorBufferFloat: false,
    timerQuery: false,
    maxTextureSize: 8192,
    maxTextureUnits: 16,
    rendererString: 'fake',
    vendorString: 'fake',
    software: false,
    ...overrides,
  };
}
